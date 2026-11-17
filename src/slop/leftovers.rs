//! `leftover-reference`: names and file paths a change removed that the working
//! tree still mentions, in code, comments, strings, or docs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};

use crate::commands::diff::declaration_changes;
use crate::git::{Base, resolve_base, run_git};
use crate::scan::outline_diff::{ChangeKind, FileStatus};
use crate::slop::types::{
    ProjectFacts, SlopConfidence, SlopEvidence, SlopFinding, SlopKind, SourceSpan,
};

/// Shortest removed name searched for; shorter ones match too much prose.
const MIN_NAME: usize = 4;
/// Mentions listed per finding; the last row counts the rest.
const MAX_ROWS: usize = 20;
/// Files larger than this are data, not prose or code.
const MAX_FILE_BYTES: u64 = 1_000_000;

const TEXT_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "vue", "svelte", "astro", "md", "mdx",
    "json", "jsonc", "yaml", "yml", "toml", "html", "css", "txt",
];
const SKIPPED_FILES: &[&str] =
    &["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "npm-shrinkwrap.json"];

/// What the change took away.
enum Gone {
    Name { name: String, file: String, renamed_to: Option<String> },
    Path { path: String },
}

impl Gone {
    fn subject(&self) -> &str {
        match self {
            Self::Name { name, .. } => name,
            Self::Path { path } => path,
        }
    }
}

pub fn detect_leftovers(root: &Path, base: &str, facts: &ProjectFacts) -> Result<Vec<SlopFinding>> {
    let base = resolve_base(root, base)?;
    let gone = gone_since(&base, facts)?;
    if gone.is_empty() {
        return Ok(Vec::new());
    }
    let files = text_files(&base)?;
    let mut findings = Vec::new();
    for item in &gone {
        let patterns = patterns(item, &base);
        let mut mentions = Vec::new();
        for (path, text) in &files {
            for pattern in &patterns {
                mentions
                    .extend(find(text, pattern).map(|offset| span(path, text, offset, pattern)));
            }
        }
        mentions.sort();
        mentions
            .dedup_by(|left, right| left.path == right.path && left.start_line == right.start_line);
        if let Some(found) = finding(item, mentions) {
            findings.push(found);
        }
    }
    Ok(findings)
}

/// Top-level names removed or renamed that no file still declares, and paths of
/// deleted or renamed files.
fn gone_since(base: &Base, facts: &ProjectFacts) -> Result<Vec<Gone>> {
    let declared = facts
        .files
        .values()
        .flat_map(|file| {
            file.declarations
                .iter()
                .map(|declaration| declaration.key.name.as_str())
                .chain(file.models.iter().map(|model| model.key.name.as_str()))
                .chain(file.export_surface.names.keys().map(String::as_str))
        })
        .collect::<BTreeSet<_>>();
    let mut gone = Vec::new();
    let mut seen = BTreeSet::new();
    for file in declaration_changes(base)? {
        match (file.status, &file.from) {
            (FileStatus::Deleted, _) => gone.push(Gone::Path { path: file.file.clone() }),
            (FileStatus::Renamed, Some(from)) => gone.push(Gone::Path { path: from.clone() }),
            _ => {}
        }
        for change in file.changes {
            let renamed_to = match change.action {
                ChangeKind::Removed => None,
                ChangeKind::Renamed => change.to.clone(),
                _ => continue,
            };
            let name = change.symbol;
            if name.contains('.')
                || name.len() < MIN_NAME
                || declared.contains(name.as_str())
                || !seen.insert(name.clone())
            {
                continue;
            }
            gone.push(Gone::Name { name, file: file.file.clone(), renamed_to });
        }
    }
    Ok(gone)
}

/// Tracked and untracked text files under the root, outside `.gitignore`.
fn text_files(base: &Base) -> Result<BTreeMap<String, String>> {
    let listed = run_git(
        &base.root,
        ["ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "."],
        "list files for leftover references",
    )?;
    let mut files = BTreeMap::new();
    for raw in listed.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
        let path = std::str::from_utf8(raw).context("Git lists a non-UTF-8 path")?;
        let file = Path::new(path);
        let name = file.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        let extension = file.extension().and_then(|extension| extension.to_str()).unwrap_or("");
        if !TEXT_EXTENSIONS.contains(&extension)
            || SKIPPED_FILES.contains(&name)
            || name.to_ascii_uppercase().starts_with("CHANGELOG")
        {
            continue;
        }
        let full = base.root.join(path);
        // Deleted but unstaged files are still listed; they have nothing to read.
        let Ok(metadata) = full.metadata() else { continue };
        if metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&full) {
            files.insert(path.to_string(), text);
        }
    }
    Ok(files)
}

struct Pattern {
    text: String,
    /// Characters that may not touch the match on either side.
    word: fn(char) -> bool,
}

fn identifier_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

fn path_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | '.')
}

fn patterns(item: &Gone, base: &Base) -> Vec<Pattern> {
    match item {
        Gone::Name { name, .. } => vec![Pattern { text: name.clone(), word: identifier_char }],
        Gone::Path { path } => {
            let prefix = base.root_prefix.to_string_lossy().replace('\\', "/");
            let mut paths = vec![path.clone()];
            if !prefix.is_empty() {
                paths.push(format!("{prefix}/{path}"));
            }
            let mut patterns = Vec::new();
            for full in paths {
                if let Some((stem, _)) = full.rsplit_once('.')
                    && !stem.ends_with('/')
                {
                    patterns.push(Pattern { text: stem.to_string(), word: path_char });
                }
                patterns.push(Pattern { text: full, word: path_char });
            }
            patterns
        }
    }
}

/// Byte offsets of whole-word matches.
fn find<'a>(text: &'a str, pattern: &'a Pattern) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(pattern.text.as_str()).filter_map(move |(offset, matched)| {
        let before = text[..offset].chars().next_back();
        let after = text[offset + matched.len()..].chars().next();
        let touches = before.is_some_and(pattern.word) || after.is_some_and(pattern.word);
        (!touches).then_some(offset)
    })
}

fn span(path: &str, text: &str, offset: usize, pattern: &Pattern) -> SourceSpan {
    let line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let line = u32::try_from(text[..offset].matches('\n').count() + 1).unwrap_or(u32::MAX);
    let column = u32::try_from(text[line_start..offset].chars().count() + 1).unwrap_or(u32::MAX);
    let width = u32::try_from(pattern.text.chars().count()).unwrap_or(u32::MAX);
    SourceSpan {
        path: path.to_string(),
        start_byte: u32::try_from(offset).unwrap_or(u32::MAX),
        end_byte: u32::try_from(offset + pattern.text.len()).unwrap_or(u32::MAX),
        start_line: line,
        start_column: column,
        end_line: line,
        end_column: column.saturating_add(width),
    }
}

fn finding(item: &Gone, mentions: Vec<SourceSpan>) -> Option<SlopFinding> {
    let total = mentions.len();
    let first = mentions.first()?.clone();
    let what = match item {
        Gone::Name { name, file, renamed_to: Some(to) } => {
            format!("`{name}` was renamed to `{to}` in `{file}`")
        }
        Gone::Name { name, file, renamed_to: None } => {
            format!("`{name}` was removed from `{file}`")
        }
        Gone::Path { path } => format!("`{path}` was deleted or moved"),
    };
    let mut evidence = mentions
        .into_iter()
        .take(MAX_ROWS)
        .map(|span| SlopEvidence {
            code: item.subject().to_string(),
            label: "mention".to_string(),
            span,
            detail: "still mentions it".to_string(),
        })
        .collect::<Vec<_>>();
    if total > MAX_ROWS
        && let Some(last) = evidence.last_mut()
    {
        last.detail = format!("still mentions it; {} more mentions not listed", total - MAX_ROWS);
    }
    let action = match item {
        Gone::Name { renamed_to: Some(to), .. } => {
            format!("Update each mention to `{to}`, or delete it.")
        }
        _ => "Update or delete each mention.".to_string(),
    };
    Some(SlopFinding {
        kind: SlopKind::LeftoverReference,
        confidence: SlopConfidence::Medium,
        span: first,
        evidence,
        explanation: if total == 1 {
            format!("{what} in this change, but 1 place still mentions it.")
        } else {
            format!("{what} in this change, but {total} places still mention it.")
        },
        action,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;
    use crate::slop::{build_project_facts, collect_project_files};

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git").args(args).current_dir(root).status().unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn reports_mentions_of_removed_names_and_deleted_paths() {
        let root = tempdir().unwrap();
        let path = root.path();
        git(path, &["init", "-q"]);
        git(path, &["config", "user.email", "scanr@example.invalid"]);
        git(path, &["config", "user.name", "scanr test"]);
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(path.join("src/orders.ts"), "export function waitingGet() { return 1; }\n")
            .unwrap();
        fs::write(path.join("src/legacy.ts"), "export function legacyThing() { return 2; }\n")
            .unwrap();
        fs::write(path.join("src/keep.ts"), "export function stillHere() { return 3; }\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-qm", "base"]);

        fs::write(path.join("src/orders.ts"), "export function waitingList() { return 1; }\n")
            .unwrap();
        fs::remove_file(path.join("src/legacy.ts")).unwrap();
        fs::write(
            path.join("NOTES.md"),
            "Call `waitingGet` first.\nSee src/legacy.ts and src/legacy-two.ts.\nwaitingGetter is different.\nstillHere stays.\n",
        )
        .unwrap();
        fs::write(path.join("CHANGELOG.md"), "Removed waitingGet.\n").unwrap();

        let facts = build_project_facts(path, collect_project_files(path).unwrap()).unwrap();
        let findings = detect_leftovers(path, "HEAD", &facts).unwrap();
        let summary = findings
            .iter()
            .map(|finding| {
                (
                    finding.evidence[0].code.clone(),
                    finding
                        .evidence
                        .iter()
                        .map(|row| format!("{}:{}", row.span.path, row.span.start_line))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                ("src/legacy.ts".to_string(), vec!["NOTES.md:2".to_string()]),
                ("waitingGet".to_string(), vec!["NOTES.md:1".to_string()]),
            ]
        );
        assert!(findings[1].action.contains("waitingList"));
    }
}
