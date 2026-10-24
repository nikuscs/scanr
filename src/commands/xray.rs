use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::{HealthSort, SlopArgs, XrayArgs};
use crate::commands::dupes::{FunctionDuplicateKey, function_duplicate_groups};
use crate::commands::tree::{
    FileHealth, HealthSeverity, build_file_health, parse_project, sort_health,
};
use crate::scan::types::FileIndex;
use crate::slop::facts::is_generated_path;
use crate::slop::output::{confidence_name, display_span, escape_markdown};
use crate::slop::types::{SlopConfidence, SlopFinding, SlopKind};

const DUPLICATE_THRESHOLD: f64 = 0.87;
const MAX_GROUP_MEMBERS_SHOWN: usize = 4;
const MAX_HOTSPOT_FINDINGS_SHOWN: usize = 3;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct XrayReport {
    version: u8,
    root: String,
    shape: Shape,
    hotspots: Vec<FileHealth>,
    slop: SlopSummary,
    duplicates: DuplicateSummary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Shape {
    files: usize,
    lines: usize,
    named_functions: usize,
    anonymous_callbacks: usize,
    high_concern_files: usize,
    medium_concern_files: usize,
    folders: Vec<FolderSize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderSize {
    path: String,
    files: usize,
    lines: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SlopSummary {
    total: usize,
    kinds: Vec<KindCount>,
    top: Vec<SlopFinding>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KindCount {
    kind: SlopKind,
    confidence: SlopConfidence,
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DuplicateSummary {
    groups: usize,
    removable_lines: usize,
    top: Vec<DuplicateGroup>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DuplicateGroup {
    removable_lines: usize,
    members: Vec<DuplicateMember>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DuplicateMember {
    path: String,
    line: u32,
    end_line: u32,
    name: String,
}

pub fn run(args: &XrayArgs) -> Result<()> {
    let report = analyze(args)?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if args.json {
        serde_json::to_writer(&mut out, &report)?;
        writeln!(out)?;
        return Ok(());
    }
    let markdown = render_markdown(&report);
    writeln!(out, "{markdown}")?;
    writeln!(out, "# ~{} tokens ({} chars)", markdown.len().div_ceil(4), markdown.len())?;
    Ok(())
}

fn analyze(args: &XrayArgs) -> Result<XrayReport> {
    let root = fs::canonicalize(&args.root).context("Cannot resolve project root")?;
    let (indices, similarity_files) = parse_project(&root, &root, false)?;
    let (indices, similarity_files): (Vec<_>, Vec<_>) = indices
        .into_iter()
        .zip(similarity_files)
        .filter(|(index, similarity)| !is_generated_path(&index.path, &similarity.source))
        .unzip();
    let duplicate_groups = function_duplicate_groups(&similarity_files, DUPLICATE_THRESHOLD, 1)?;

    let mut files: Vec<FileHealth> = indices
        .iter()
        .zip(&similarity_files)
        .map(|(index, similarity)| build_file_health(index, similarity, &duplicate_groups))
        .collect();
    sort_health(&mut files, HealthSort::Severity);

    let shape = build_shape(&files, args.top);
    let duplicates = build_duplicates(&indices, &duplicate_groups, args.top);
    let slop = build_slop(&root, args.top)?;
    files.retain(|file| file.severity != HealthSeverity::None);
    files.truncate(args.top);

    Ok(XrayReport {
        version: 1,
        root: root.to_string_lossy().to_string(),
        shape,
        hotspots: files,
        slop,
        duplicates,
    })
}

fn build_shape(files: &[FileHealth], top: usize) -> Shape {
    let mut folders: BTreeMap<String, FolderSize> = BTreeMap::new();
    for file in files {
        let folder = Path::new(&file.path)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .filter(|parent| !parent.is_empty())
            .unwrap_or_else(|| ".".to_string());
        let entry = folders.entry(folder.clone()).or_insert(FolderSize {
            path: folder,
            files: 0,
            lines: 0,
        });
        entry.files += 1;
        entry.lines += file.lines;
    }
    let mut folders: Vec<_> = folders.into_values().collect();
    folders.sort_by(|left, right| right.lines.cmp(&left.lines).then(left.path.cmp(&right.path)));
    folders.truncate(top);

    Shape {
        files: files.len(),
        lines: files.iter().map(|file| file.lines).sum(),
        named_functions: files.iter().map(|file| file.named_functions).sum(),
        anonymous_callbacks: files.iter().map(|file| file.anonymous_callbacks).sum(),
        high_concern_files: count_severity(files, HealthSeverity::High),
        medium_concern_files: count_severity(files, HealthSeverity::Medium),
        folders,
    }
}

fn count_severity(files: &[FileHealth], severity: HealthSeverity) -> usize {
    files.iter().filter(|file| file.severity == severity).count()
}

fn build_slop(root: &Path, top: usize) -> Result<SlopSummary> {
    let report = crate::commands::slop::analyze(&SlopArgs {
        root: root.to_string_lossy().to_string(),
        base: None,
        confidence: SlopConfidence::Medium,
        only: Vec::new(),
        exclude: Vec::new(),
        top: None,
        include_test_files: false,
        oversplit_max_lines: 30,
        oversplit_allow_importer: Vec::new(),
        member_factory: vec!["make*".to_string()],
        type_aware: false,
        tsconfig: Vec::new(),
        json: true,
    })?;

    let mut counts: BTreeMap<SlopKind, KindCount> = BTreeMap::new();
    for finding in &report.findings {
        counts
            .entry(finding.kind)
            .or_insert(KindCount { kind: finding.kind, confidence: finding.confidence, count: 0 })
            .count += 1;
    }
    let mut kinds: Vec<_> = counts.into_values().collect();
    kinds.sort_by(|left, right| {
        left.confidence
            .cmp(&right.confidence)
            .then(right.count.cmp(&left.count))
            .then(left.kind.cmp(&right.kind))
    });

    let total = report.findings.len();
    let mut findings = report.findings;
    findings.sort_by_key(|finding| finding.confidence);
    findings.truncate(top);
    Ok(SlopSummary { total, kinds, top: findings })
}

fn build_duplicates(
    indices: &[FileIndex],
    duplicate_groups: &BTreeMap<FunctionDuplicateKey, Vec<FunctionDuplicateKey>>,
    top: usize,
) -> DuplicateSummary {
    let end_lines: BTreeMap<FunctionDuplicateKey, u32> = indices
        .iter()
        .flat_map(|index| {
            index.functions.iter().filter_map(|function| {
                let name = function.name.clone()?;
                Some(((index.path.clone(), function.line, name), function.line_end))
            })
        })
        .collect();

    let mut groups: Vec<DuplicateGroup> = duplicate_groups
        .iter()
        .filter(|(key, group)| group.first() == Some(*key))
        .map(|(_, group)| {
            let members: Vec<_> = group
                .iter()
                .map(|key| DuplicateMember {
                    path: key.0.clone(),
                    line: key.1,
                    end_line: end_lines.get(key).copied().unwrap_or(key.1),
                    name: key.2.clone(),
                })
                .collect();
            let removable_lines = members
                .iter()
                .skip(1)
                .map(|member| (member.end_line.saturating_sub(member.line) + 1) as usize)
                .sum();
            DuplicateGroup { removable_lines, members }
        })
        .collect();
    groups.sort_by(|left, right| {
        right
            .removable_lines
            .cmp(&left.removable_lines)
            .then(left.members[0].path.cmp(&right.members[0].path))
            .then(left.members[0].line.cmp(&right.members[0].line))
    });

    let summary_groups = groups.len();
    let removable_lines = groups.iter().map(|group| group.removable_lines).sum();
    groups.truncate(top);
    DuplicateSummary { groups: summary_groups, removable_lines, top: groups }
}

fn render_markdown(report: &XrayReport) -> String {
    let mut lines = Vec::new();
    let name = Path::new(&report.root)
        .file_name()
        .map_or_else(|| report.root.clone(), |name| name.to_string_lossy().to_string());
    lines.push(format!("# Xray: {name}"));
    lines.push(String::new());
    render_shape(&report.shape, &mut lines);
    render_hotspots(&report.hotspots, &mut lines);
    render_slop(&report.slop, &mut lines);
    render_duplicates(&report.duplicates, &mut lines);
    lines.join("\n")
}

fn render_shape(shape: &Shape, lines: &mut Vec<String>) {
    lines.push("## Shape".to_string());
    lines.push(String::new());
    lines.push(format!(
        "{} source files, {} lines, {} named functions, {} anonymous callbacks (tests and generated files excluded). \
         Health: {} high concern, {} medium concern.",
        shape.files,
        shape.lines,
        shape.named_functions,
        shape.anonymous_callbacks,
        shape.high_concern_files,
        shape.medium_concern_files
    ));
    lines.push(String::new());
    lines.push("| Folder | Files | Lines |".to_string());
    lines.push("| --- | --- | --- |".to_string());
    for folder in &shape.folders {
        lines.push(format!(
            "| {} | {} | {} |",
            escape_markdown(&folder.path),
            folder.files,
            folder.lines
        ));
    }
    lines.push(String::new());
}

fn render_hotspots(hotspots: &[FileHealth], lines: &mut Vec<String>) {
    lines.push("## Hotspots".to_string());
    lines.push(String::new());
    if hotspots.is_empty() {
        lines.push("No file crosses a health threshold.".to_string());
        lines.push(String::new());
        return;
    }
    lines.push("| File | Concern | Lines | Findings |".to_string());
    lines.push("| --- | --- | --- | --- |".to_string());
    for file in hotspots {
        let mut findings = file.findings.iter().collect::<Vec<_>>();
        findings.sort_by_key(|finding| std::cmp::Reverse(finding.severity));
        let mut messages = findings
            .iter()
            .take(MAX_HOTSPOT_FINDINGS_SHOWN)
            .map(|finding| finding.message.clone())
            .collect::<Vec<_>>();
        let hidden = findings.len().saturating_sub(MAX_HOTSPOT_FINDINGS_SHOWN);
        if hidden > 0 {
            messages.push(format!("+{hidden} more"));
        }
        lines.push(format!(
            "| {} | {} | {} | {} |",
            escape_markdown(&file.path),
            file.severity.label(),
            file.lines,
            escape_markdown(&messages.join("; "))
        ));
    }
    lines.push(String::new());
}

fn render_slop(slop: &SlopSummary, lines: &mut Vec<String>) {
    lines.push(format!(
        "## Slop ({} finding{})",
        slop.total,
        if slop.total == 1 { "" } else { "s" }
    ));
    lines.push(String::new());
    if slop.total == 0 {
        lines.push("No slop findings.".to_string());
        lines.push(String::new());
        return;
    }
    lines.push("| Kind | Confidence | Count |".to_string());
    lines.push("| --- | --- | --- |".to_string());
    for kind in &slop.kinds {
        lines.push(format!(
            "| {} | {} | {} |",
            kind.kind.cli_name(),
            confidence_name(kind.confidence),
            kind.count
        ));
    }
    lines.push(String::new());
    lines.push("| File | Finding | Confidence | Suggested action |".to_string());
    lines.push("| --- | --- | --- | --- |".to_string());
    for finding in &slop.top {
        lines.push(format!(
            "| {} | {} | {} | {} |",
            escape_markdown(&display_span(&finding.span)),
            escape_markdown(finding.kind.display_name()),
            confidence_name(finding.confidence),
            escape_markdown(&finding.action)
        ));
    }
    lines.push(String::new());
    lines.push("Run `scanr slop` for evidence on every finding.".to_string());
    lines.push(String::new());
}

fn render_duplicates(duplicates: &DuplicateSummary, lines: &mut Vec<String>) {
    lines.push(format!(
        "## Duplicates ({} group{}, about {} removable lines)",
        duplicates.groups,
        if duplicates.groups == 1 { "" } else { "s" },
        duplicates.removable_lines
    ));
    lines.push(String::new());
    if duplicates.groups == 0 {
        lines.push("No similar function groups.".to_string());
        return;
    }
    lines.push("| Removable lines | Functions |".to_string());
    lines.push("| --- | --- |".to_string());
    for group in &duplicates.top {
        let mut members = group
            .members
            .iter()
            .take(MAX_GROUP_MEMBERS_SHOWN)
            .map(|member| format!("{}:{} {}", member.path, member.line, member.name))
            .collect::<Vec<_>>();
        let hidden = group.members.len().saturating_sub(MAX_GROUP_MEMBERS_SHOWN);
        if hidden > 0 {
            members.push(format!("+{hidden} more"));
        }
        lines.push(format!(
            "| {} | {} |",
            group.removable_lines,
            escape_markdown(&members.join(", "))
        ));
    }
    lines.push(String::new());
    lines.push("Run `scanr dupes` for pair similarity scores.".to_string());
}
