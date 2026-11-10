//! `scanr diff`: what a change did to the project's declarations, compared
//! with a Git base revision.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use rayon::prelude::*;
use serde::Serialize;

use crate::cli::DiffArgs;
use crate::git::{Base, resolve_base, run_git};
use crate::scan::outline::{FileOutline, OutlineView, SymbolKind};
use crate::scan::outline_diff::{
    Change, ChangeKind, FileDiff, FileStatus, FileVersions, diff_outlines,
};
use crate::scan::typescript::parse::outline_source;
use crate::scan::{ScanConfig, has_matching_extension};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffReport {
    version: u8,
    base: String,
    resolved_base: String,
    files: Vec<FileDiff>,
}

pub fn run(args: &DiffArgs) -> Result<()> {
    let base = resolve_base(Path::new(&args.root), &args.base)?;
    let view = OutlineView { deep: args.deep, signatures: true, exported: false };
    let files = changed_files(&base)?
        .into_par_iter()
        .map(|file| load_versions(&base, file, view))
        .collect::<Result<Vec<_>>>()?;
    let report = DiffReport {
        version: 1,
        base: args.base.clone(),
        resolved_base: base.commit,
        files: diff_outlines(&files),
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if args.json {
        serde_json::to_writer(&mut out, &report)?;
        writeln!(out)?;
    } else {
        write!(out, "{}", render(&report))?;
    }
    Ok(())
}

struct ChangedFile {
    path: String,
    status: FileStatus,
    from: Option<String>,
}

/// Scannable files that differ from the base: tracked changes in the working
/// tree plus untracked files, which count as added.
fn changed_files(base: &Base) -> Result<Vec<ChangedFile>> {
    let tracked = run_git(
        &base.root,
        [
            "diff",
            "--relative",
            "--name-status",
            "-z",
            "--find-renames",
            base.commit.as_str(),
            "--",
            ".",
        ],
        "enumerate Git diff files",
    )?;
    let untracked = run_git(
        &base.root,
        ["ls-files", "--others", "--exclude-standard", "-z", "--", "."],
        "enumerate untracked files",
    )?;

    let mut files = parse_name_status(&tracked.stdout)?;
    for path in untracked.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
        let path = std::str::from_utf8(path).context("Git lists a non-UTF-8 path")?;
        files.push(ChangedFile { path: path.to_string(), status: FileStatus::Added, from: None });
    }

    let extensions = ScanConfig::default().extensions;
    let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
    files.retain(|file| has_matching_extension(Path::new(&file.path), &extensions));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    Ok(files)
}

fn parse_name_status(bytes: &[u8]) -> Result<Vec<ChangedFile>> {
    let mut fields = bytes.split(|byte| *byte == 0).filter(|field| !field.is_empty());
    let next_path = |fields: &mut dyn Iterator<Item = &[u8]>| -> Result<String> {
        let path = fields.next().ok_or_else(|| anyhow!("Git name-status record is truncated"))?;
        Ok(std::str::from_utf8(path).context("Git diff contains a non-UTF-8 path")?.to_string())
    };
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let first = next_path(&mut fields)?;
        let file = match status.first() {
            Some(b'A') => ChangedFile { path: first, status: FileStatus::Added, from: None },
            Some(b'D') => ChangedFile { path: first, status: FileStatus::Deleted, from: None },
            Some(b'R') => ChangedFile {
                path: next_path(&mut fields)?,
                status: FileStatus::Renamed,
                from: Some(first),
            },
            Some(b'C') => {
                // A copy leaves its source in place: the destination is a new file.
                ChangedFile { path: next_path(&mut fields)?, status: FileStatus::Added, from: None }
            }
            _ => ChangedFile { path: first, status: FileStatus::Modified, from: None },
        };
        files.push(file);
    }
    Ok(files)
}

fn load_versions(base: &Base, file: ChangedFile, view: OutlineView) -> Result<FileVersions> {
    let outline = |path: &str, source: &str| -> Result<FileOutline> {
        Ok(outline_source(Path::new(path), source)?.view(view))
    };
    let before = if file.status == FileStatus::Added {
        None
    } else {
        let path = file.from.as_deref().unwrap_or(&file.path);
        Some(outline(path, &base.read(path)?)?)
    };
    let after = if file.status == FileStatus::Deleted {
        None
    } else {
        let source = std::fs::read_to_string(base.root.join(&file.path))
            .with_context(|| format!("failed to read {}", file.path))?;
        Some(outline(&file.path, &source)?)
    };
    Ok(FileVersions { path: file.path, status: file.status, from: file.from, before, after })
}

fn render(report: &DiffReport) -> String {
    let short_base: String = report.resolved_base.chars().take(7).collect();
    let mut out = format!("# Outline diff against {} ({short_base})\n", report.base);
    if report.files.is_empty() {
        out.push_str("\nNo declaration changes.\n");
        return out;
    }
    for file in &report.files {
        out.push('\n');
        out.push_str(&file.file);
        match (file.status, &file.from) {
            (FileStatus::Added, _) => out.push_str("  (new file)"),
            (FileStatus::Deleted, _) => out.push_str("  (deleted)"),
            (FileStatus::Renamed, Some(from)) => out.push_str(&format!("  (renamed from {from})")),
            _ => {}
        }
        out.push('\n');
        let rows: Vec<(String, String)> = file.changes.iter().map(change_row).collect();
        let width = rows.iter().map(|(label, _)| label.chars().count()).max().unwrap_or(0);
        for (label, note) in rows {
            let padding = " ".repeat(width - label.chars().count());
            out.push_str(format!("  {label}{padding}  {note}").trim_end());
            out.push('\n');
        }
    }
    out
}

fn change_row(change: &Change) -> (String, String) {
    // `()` marks a function; other kinds are named, since a const and a type can share a name.
    let call = |path: &str| match change.kind {
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor => format!("{path}()"),
        SymbolKind::Const => path.to_string(),
        kind => format!("{path} {}", kind.as_str()),
    };
    let symbol = call(&change.symbol);
    let details = change.details.join("; ");
    match change.action {
        ChangeKind::Added => (format!("+ {symbol}"), "added".to_string()),
        ChangeKind::Removed => (format!("- {symbol}"), "removed".to_string()),
        ChangeKind::Changed => (format!("~ {symbol}"), details),
        ChangeKind::Renamed => {
            let to = call(change.to.as_deref().unwrap_or_default());
            let note = if details.is_empty() {
                "renamed".to_string()
            } else {
                format!("renamed; {details}")
            };
            (format!("~ {symbol} → {to}"), note)
        }
        ChangeKind::Moved => {
            let to = call(change.to.as_deref().unwrap_or_default());
            let note = change
                .to_file
                .as_ref()
                .map_or_else(|| "moved".to_string(), |file| format!("moved to {file}"));
            (format!("> {symbol} → {to}"), note)
        }
    }
}

#[cfg(test)]
#[path = "diff_test.rs"]
mod tests;
