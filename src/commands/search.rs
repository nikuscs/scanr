use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use fff_grep::{LineTerminator, Match, Matcher, Searcher, Sink, SinkMatch};
use ignore::{WalkBuilder, overrides::OverrideBuilder};
use rayon::prelude::*;
use serde::Serialize;

use crate::cli::SearchArgs;

#[derive(Debug, Serialize)]
struct SearchMatch {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    before: Vec<ContextLine>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    after: Vec<ContextLine>,
}

#[derive(Debug, Serialize)]
struct ContextLine {
    line: u64,
    text: String,
}

struct LiteralMatcher {
    needle: Vec<u8>,
    ignore_case: bool,
}

impl Matcher for &LiteralMatcher {
    type Error = io::Error;

    fn find_at(&self, haystack: &[u8], at: usize) -> Result<Option<Match>, Self::Error> {
        if at >= haystack.len() || self.needle.len() > haystack.len().saturating_sub(at) {
            return Ok(None);
        }

        let found = haystack[at..]
            .windows(self.needle.len())
            .position(|window| bytes_equal(window, &self.needle, self.ignore_case));
        Ok(found.map(|offset| {
            let start = at + offset;
            Match::new(start, start + self.needle.len())
        }))
    }

    fn line_terminator(&self) -> Option<LineTerminator> {
        Some(LineTerminator::byte(b'\n'))
    }
}

struct MatchSink {
    lines: Vec<u64>,
    max_count: Option<usize>,
}

impl Sink for &mut MatchSink {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if let Some(line) = mat.line_number() {
            self.lines.push(line);
        }
        Ok(self.max_count.is_none_or(|max| self.lines.len() < max))
    }
}

pub fn run(args: &SearchArgs) -> Result<()> {
    let root = fs::canonicalize(&args.root).context("Cannot resolve project root")?;
    let files = collect_files(&root, &args.glob)?;

    let mut matches = if let Some(pattern) = args.path.as_deref() {
        search_paths(&root, &files, pattern, args.ignore_case, args.max_count)
    } else {
        let pattern = args.pattern.as_deref().unwrap_or_default();
        if pattern.is_empty() {
            bail!("search pattern must not be empty");
        }
        search_contents(&root, &files, pattern, args.ignore_case, args.max_count, args.context)
    };

    matches
        .sort_by(|left, right| left.path.cmp(&right.path).then_with(|| left.line.cmp(&right.line)));
    write_matches(&matches, args.json)
}

fn collect_files(root: &Path, globs: &[String]) -> Result<Vec<PathBuf>> {
    let mut overrides = OverrideBuilder::new(root);
    for glob in globs {
        overrides.add(glob).with_context(|| format!("Invalid glob: {glob}"))?;
    }

    let mut builder = WalkBuilder::new(root);
    builder.standard_filters(true).hidden(false);
    if !globs.is_empty() {
        builder.overrides(overrides.build().context("Cannot build glob filters")?);
    }

    let mut files: Vec<_> = builder
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(ignore::DirEntry::into_path)
        .collect();
    files.sort();
    Ok(files)
}

fn search_contents(
    root: &Path,
    files: &[PathBuf],
    pattern: &str,
    ignore_case: bool,
    max_count: Option<usize>,
    context: usize,
) -> Vec<SearchMatch> {
    let matcher = LiteralMatcher { needle: pattern.as_bytes().to_vec(), ignore_case };

    files
        .par_iter()
        .filter_map(|path| {
            let bytes = fs::read(path).ok()?;
            if bytes.iter().take(1024).any(|byte| *byte == 0) {
                return None;
            }

            let mut sink = MatchSink { lines: Vec::new(), max_count };
            Searcher::new().search_slice(&matcher, &bytes, &mut sink).ok()?;
            if sink.lines.is_empty() {
                return None;
            }

            let lines: Vec<_> = bytes
                .split(|byte| *byte == b'\n')
                .map(|line| {
                    String::from_utf8_lossy(line.strip_suffix(b"\r").unwrap_or(line)).into()
                })
                .collect();
            let rel_path = relative_path(root, path);
            Some(
                sink.lines
                    .into_iter()
                    .map(|line| content_match(&rel_path, line, &lines, context))
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

fn content_match(path: &str, line: u64, lines: &[String], context: usize) -> SearchMatch {
    let index = usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX);
    let before_start = index.saturating_sub(context);
    let after_end = (index + context + 1).min(lines.len());
    let context_line = |offset: usize| ContextLine {
        line: u64::try_from(offset + 1).unwrap_or(u64::MAX),
        text: lines[offset].clone(),
    };

    SearchMatch {
        path: path.to_string(),
        line: Some(line),
        text: lines.get(index).cloned(),
        before: (before_start..index).map(context_line).collect(),
        after: ((index + 1).min(lines.len())..after_end).map(context_line).collect(),
    }
}

fn search_paths(
    root: &Path,
    files: &[PathBuf],
    pattern: &str,
    ignore_case: bool,
    max_count: Option<usize>,
) -> Vec<SearchMatch> {
    let mut matches: Vec<_> = files
        .iter()
        .map(|path| relative_path(root, path))
        .filter(|path| path_matches(path, pattern, ignore_case))
        .map(|path| SearchMatch {
            path,
            line: None,
            text: None,
            before: Vec::new(),
            after: Vec::new(),
        })
        .collect();
    if let Some(max) = max_count {
        matches.truncate(max);
    }
    matches
}

fn bytes_equal(left: &[u8], right: &[u8], ignore_case: bool) -> bool {
    if ignore_case {
        left.iter().zip(right).all(|(left, right)| left.eq_ignore_ascii_case(right))
    } else {
        left == right
    }
}

fn path_matches(path: &str, pattern: &str, ignore_case: bool) -> bool {
    let (path, pattern) = if ignore_case {
        (path.to_ascii_lowercase(), pattern.to_ascii_lowercase())
    } else {
        (path.to_string(), pattern.to_string())
    };

    if pattern.contains(['*', '?']) {
        wildcard_match(path.as_bytes(), pattern.as_bytes())
    } else {
        path.contains(&pattern)
    }
}

fn wildcard_match(value: &[u8], pattern: &[u8]) -> bool {
    let (mut value_index, mut pattern_index) = (0, 0);
    let (mut star, mut retry_value) = (None, 0);

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            value_index += 1;
            pattern_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            retry_value = value_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            retry_value += 1;
            value_index = retry_value;
        } else {
            return false;
        }
    }

    pattern[pattern_index..].iter().all(|byte| *byte == b'*')
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn write_matches(matches: &[SearchMatch], json: bool) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if json {
        serde_json::to_writer(&mut out, matches)?;
        writeln!(out)?;
        return Ok(());
    }

    for matched in matches {
        if let Some(line) = matched.line {
            for context in &matched.before {
                writeln!(out, "{}-{}-{}", matched.path, context.line, context.text)?;
            }
            writeln!(out, "{}:{}:{}", matched.path, line, matched.text.as_deref().unwrap_or(""))?;
            for context in &matched.after {
                writeln!(out, "{}-{}-{}", matched.path, context.line, context.text)?;
            }
        } else {
            writeln!(out, "{}", matched.path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_matcher_supports_case_modes() {
        let exact = LiteralMatcher { needle: b"State".to_vec(), ignore_case: false };
        assert_eq!((&exact).find(b"useState").unwrap(), Some(Match::new(3, 8)));
        assert_eq!((&exact).find(b"usestate").unwrap(), None);

        let folded = LiteralMatcher { needle: b"State".to_vec(), ignore_case: true };
        assert_eq!((&folded).find(b"usestate").unwrap(), Some(Match::new(3, 8)));
    }

    #[test]
    fn wildcard_path_matching_is_plain_and_deterministic() {
        assert!(path_matches("src/components/app.tsx", "*components/*.tsx", false));
        assert!(path_matches("src/App.TSX", "*.tsx", true));
        assert!(!path_matches("src/app.ts", "*.tsx", false));
    }
}
