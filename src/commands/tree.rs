use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Serialize;

use crate::cli::HealthSort;
use crate::commands::dupes::{FunctionDuplicateKey, function_duplicate_groups};
use crate::scan::rules::{hoistable_function, is_test_path, low_value_function};
use crate::scan::types::{FileIndex, FunctionInfo, FunctionKindsFilter};
use crate::scan::typescript::parse::{SimilarityFile, process_file_with_similarity};
use crate::scan::{ScanConfig, collect_files};

const IGNORE_DIRS: &[&str] = &[
    ".git",
    ".superset",
    ".glooit",
    ".claude",
    ".codex",
    "node_modules",
    "dist",
    "build",
    "target",
    ".next",
    ".turbo",
    "coverage",
];

const IGNORE_TEST_DIRS: &[&str] = &["test", "tests", "__test__", "__tests__"];

const INCLUDE_FILE_NAMES: &[&str] = &["Dockerfile", "LICENSE", "Makefile", "Procfile", "README"];

const INCLUDE_EXTS: &[&str] = &[
    "cjs", "cts", "css", "env", "go", "html", "java", "js", "json", "jsx", "md", "mdx", "mjs",
    "mts", "py", "rs", "scss", "sh", "sql", "toml", "ts", "tsx", "txt", "yaml", "yml",
];

const STRIP_EXTS: &[&str] =
    &["cjs", "cts", "go", "java", "js", "jsx", "mjs", "mts", "py", "rs", "ts", "tsx"];

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
pub fn run(
    root: &str,
    subpath: Option<&str>,
    depth: usize,
    inline: usize,
    all: bool,
    functions: bool,
    all_functions: bool,
    low_value_max_lines: u32,
    duplicate_threshold: f64,
    function_details: bool,
    function_min_lines: u32,
    function_max_lines: u32,
    health: bool,
    only_findings: bool,
    top: Option<usize>,
    sort_by: Option<HealthSort>,
    json: bool,
) -> Result<()> {
    let project =
        fs::canonicalize(root).context("Cannot resolve project root")?.display().to_string();
    let project_root = PathBuf::from(&project);

    let start_rel = subpath.unwrap_or("").trim_matches('/');
    let start_path =
        if start_rel.is_empty() { project_root.clone() } else { project_root.join(start_rel) };

    let start_path = fs::canonicalize(&start_path)
        .with_context(|| format!("Cannot resolve path {start_rel}"))?;

    if !start_path.starts_with(&project_root) {
        anyhow::bail!("Path must be inside the project root");
    }

    if !(0.0..=1.0).contains(&duplicate_threshold) {
        anyhow::bail!("Duplicate threshold must be between 0 and 1");
    }
    if function_details && function_min_lines == 0 {
        anyhow::bail!("Function minimum lines must be at least 1");
    }
    if function_details && function_min_lines > function_max_lines {
        anyhow::bail!("Function minimum lines cannot exceed maximum lines");
    }

    let annotations = if functions || health {
        build_function_annotations(
            &start_path,
            &project_root,
            all_functions,
            low_value_max_lines,
            duplicate_threshold,
            health,
            all,
        )?
    } else {
        BTreeMap::new()
    };
    let tree = build_node(&start_path, &project_root, all, &annotations)?;
    if health {
        return write_health_report(
            &tree,
            &project,
            only_findings,
            top,
            sort_by.unwrap_or(HealthSort::Severity),
            json,
        );
    }

    let mut lines = Vec::new();
    lines.push("# Project Structure".to_string());
    lines.push(String::new());
    if function_details {
        render_detailed_tree(
            &tree,
            &mut lines,
            depth.max(1),
            function_min_lines,
            function_max_lines,
        );
    } else {
        render_node(&tree, &mut lines, "", true, 0, depth.max(1), inline.max(1));
    }

    let output = lines.join("\n");
    let chars = output.len();
    let estimated_tokens = chars.div_ceil(4);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "{output}")?;
    writeln!(out)?;
    writeln!(out, "# ~{estimated_tokens} tokens ({chars} chars, {} lines)", lines.len())?;
    Ok(())
}

fn build_function_annotations(
    scan_root: &Path,
    project_root: &Path,
    all_functions: bool,
    low_value_max_lines: u32,
    duplicate_threshold: f64,
    include_health: bool,
    include_tests: bool,
) -> Result<BTreeMap<String, FunctionAnnotations>> {
    let files = collect_files(scan_root, &ScanConfig::default())?;
    let files: Vec<_> = files
        .into_iter()
        .filter(|path| include_tests || !is_test_path(&rel_path(project_root, path)))
        .collect();
    let parsed: Vec<Result<_>> = files
        .par_iter()
        .map(|path| process_file_with_similarity(path, project_root, FunctionKindsFilter::All))
        .collect();
    let parsed = parsed.into_iter().collect::<Result<Vec<_>>>()?;
    let (indices, similarity_files): (Vec<_>, Vec<_>) = parsed.into_iter().unzip();
    let duplicate_groups = function_duplicate_groups(&similarity_files, duplicate_threshold, 1)?;
    let similarity_by_path: BTreeMap<_, _> =
        similarity_files.iter().map(|file| (file.path.as_str(), file)).collect();
    let qualified_names: BTreeMap<_, _> = indices
        .iter()
        .flat_map(|index| {
            index.functions.iter().filter_map(|function| {
                let name = function.name.as_deref()?;
                let qualified = function
                    .parent
                    .as_ref()
                    .map_or_else(|| name.to_string(), |parent| format!("{parent}.{name}"));
                Some(((index.path.clone(), function.line, name.to_string()), qualified))
            })
        })
        .collect();

    let mut annotations = BTreeMap::new();
    for index in indices {
        let test_file = is_test_path(&index.path);
        let entry =
            annotations.entry(index.path.clone()).or_insert_with(FunctionAnnotations::default);
        for function in &index.functions {
            let Some(name) = function.name.as_deref() else {
                if all_functions {
                    entry.labels.push(function_label(
                        function,
                        format!("<anonymous@{}>", function.line),
                        test_file,
                        low_value_max_lines,
                        Vec::new(),
                    ));
                } else {
                    entry.anonymous_count += 1;
                }
                continue;
            };
            let qualified = function
                .parent
                .as_ref()
                .map_or_else(|| name.to_string(), |parent| format!("{parent}.{name}"));
            let key = (index.path.clone(), function.line, name.to_string());
            let duplicate_peers = duplicate_groups
                .get(&key)
                .into_iter()
                .flatten()
                .filter(|peer| *peer != &key)
                .map(|peer| SimilarityPeer {
                    path: peer.0.clone(),
                    line: peer.1,
                    name: qualified_names.get(peer).cloned().unwrap_or_else(|| peer.2.clone()),
                })
                .collect();
            entry.labels.push(function_label(
                function,
                qualified,
                test_file,
                low_value_max_lines,
                duplicate_peers,
            ));
        }
        entry.labels.sort_by(|left, right| {
            left.line.cmp(&right.line).then_with(|| left.name.cmp(&right.name))
        });
        if include_health && let Some(similarity) = similarity_by_path.get(index.path.as_str()) {
            entry.health = Some(build_file_health(&index, similarity, &duplicate_groups));
        }
    }
    Ok(annotations)
}

fn function_label(
    function: &FunctionInfo,
    name: String,
    test_file: bool,
    low_value_max_lines: u32,
    duplicate_peers: Vec<SimilarityPeer>,
) -> FunctionLabel {
    let mut markers = Vec::new();
    if !test_file && hoistable_function(function) {
        markers.push("[H]".to_string());
    }
    if !function.captures.is_empty() {
        markers.push(format!("[C:{}]", function.captures.len()));
    }
    if !test_file && low_value_function(function, low_value_max_lines) {
        markers.push("[L]".to_string());
    }
    if !duplicate_peers.is_empty() {
        markers.push(format!("[D:{}]", duplicate_peers.len() + 1));
    }
    FunctionLabel {
        line: function.line,
        lines: function.line_end.saturating_sub(function.line) + 1,
        name,
        parent: function.parent.clone(),
        capture_count: function.captures.len(),
        hoistable: !test_file && hoistable_function(function),
        low_value_reason: function.low_value_reason.clone(),
        duplicate_peers,
        markers,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SimilarityPeer {
    path: String,
    line: u32,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionLabel {
    line: u32,
    lines: u32,
    name: String,
    parent: Option<String>,
    capture_count: usize,
    hoistable: bool,
    low_value_reason: Option<String>,
    duplicate_peers: Vec<SimilarityPeer>,
    markers: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FunctionAnnotations {
    labels: Vec<FunctionLabel>,
    anonymous_count: usize,
    health: Option<FileHealth>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
enum HealthSeverity {
    None,
    Medium,
    High,
}

impl HealthSeverity {
    const fn label(self) -> &'static str {
        match self {
            Self::None => "OK",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthFinding {
    kind: String,
    severity: HealthSeverity,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileHealth {
    path: String,
    severity: HealthSeverity,
    lines: usize,
    largest_function: Option<String>,
    largest_function_lines: u32,
    named_functions: usize,
    anonymous_callbacks: usize,
    nested_functions: usize,
    max_function_nesting: usize,
    capture_total: usize,
    capture_max: usize,
    trivial_wrappers: usize,
    wrapper_density_percent: usize,
    duplicate_groups: usize,
    duplicate_functions: usize,
    estimated_removable_lines: usize,
    max_branch_complexity: usize,
    max_control_nesting: usize,
    hook_calls: usize,
    state_calls: usize,
    effect_calls: usize,
    exports: usize,
    dependencies: usize,
    findings: Vec<HealthFinding>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthReport {
    version: u8,
    root: String,
    files: Vec<FileHealth>,
}

fn build_file_health(
    index: &FileIndex,
    similarity: &SimilarityFile,
    duplicate_groups: &BTreeMap<FunctionDuplicateKey, Vec<FunctionDuplicateKey>>,
) -> FileHealth {
    let named_functions = index.functions.iter().filter(|function| function.name.is_some()).count();
    let anonymous_callbacks = index.functions.len().saturating_sub(named_functions);
    let nested_functions =
        index.functions.iter().filter(|function| function.parent.is_some()).count();
    let max_function_nesting = index
        .functions
        .iter()
        .filter_map(|function| function.parent.as_ref())
        .map(|parent| parent.matches('.').count() + 1)
        .max()
        .unwrap_or(0);
    let capture_total = index.functions.iter().map(|function| function.captures.len()).sum();
    let capture_max =
        index.functions.iter().map(|function| function.captures.len()).max().unwrap_or(0);
    let trivial_wrappers = index
        .functions
        .iter()
        .filter(|function| function.name.is_some() && function.low_value_reason.is_some())
        .count();
    let wrapper_density_percent =
        trivial_wrappers.saturating_mul(100).checked_div(named_functions).unwrap_or(0);
    let largest = index
        .functions
        .iter()
        .filter_map(|function| {
            let name = function.name.as_ref()?;
            Some((function.line_end.saturating_sub(function.line) + 1, name.clone()))
        })
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));
    let (largest_function_lines, largest_function) =
        largest.map_or((0, None), |(lines, name)| (lines, Some(name)));

    let mut group_ids = BTreeSet::new();
    let mut duplicate_functions = 0;
    let mut estimated_removable_lines = 0;
    for function in &index.functions {
        let Some(name) = function.name.as_ref() else {
            continue;
        };
        let key = (index.path.clone(), function.line, name.clone());
        let Some(group) = duplicate_groups.get(&key) else {
            continue;
        };
        let Some(canonical) = group.first() else {
            continue;
        };
        group_ids.insert(canonical.clone());
        duplicate_functions += 1;
        if &key != canonical {
            estimated_removable_lines +=
                function.line_end.saturating_sub(function.line) as usize + 1;
        }
    }

    let mut health = FileHealth {
        path: index.path.clone(),
        severity: HealthSeverity::None,
        lines: similarity.source.lines().count(),
        largest_function,
        largest_function_lines,
        named_functions,
        anonymous_callbacks,
        nested_functions,
        max_function_nesting,
        capture_total,
        capture_max,
        trivial_wrappers,
        wrapper_density_percent,
        duplicate_groups: group_ids.len(),
        duplicate_functions,
        estimated_removable_lines,
        max_branch_complexity: similarity.health.max_branch_complexity,
        max_control_nesting: similarity.health.max_control_nesting,
        hook_calls: similarity.health.hook_calls,
        state_calls: similarity.health.state_calls,
        effect_calls: similarity.health.effect_calls,
        exports: index.exports.len(),
        dependencies: similarity.health.dependencies.len(),
        findings: Vec::new(),
    };
    health.findings = health_findings(&health);
    health.severity = health
        .findings
        .iter()
        .map(|finding| finding.severity)
        .max()
        .unwrap_or(HealthSeverity::None);
    health
}

#[allow(clippy::too_many_lines)]
fn health_findings(health: &FileHealth) -> Vec<HealthFinding> {
    let mut findings = Vec::new();
    push_threshold_finding(
        &mut findings,
        "oversized_file",
        health.lines,
        300,
        500,
        format!("{} lines", health.lines),
    );
    push_threshold_finding(
        &mut findings,
        "function_sprawl",
        health.named_functions + health.anonymous_callbacks,
        15,
        30,
        format!(
            "{} named functions and {} anonymous callbacks",
            health.named_functions, health.anonymous_callbacks
        ),
    );
    push_threshold_finding(
        &mut findings,
        "callback_sprawl",
        health.anonymous_callbacks,
        10,
        20,
        format!("{} anonymous callbacks", health.anonymous_callbacks),
    );
    push_threshold_finding(
        &mut findings,
        "capture_coupling",
        health.capture_total,
        15,
        30,
        format!(
            "{} total parent-variable uses; maximum {} in one function",
            health.capture_total, health.capture_max
        ),
    );
    if health.trivial_wrappers >= 3 && health.wrapper_density_percent >= 30 {
        findings.push(HealthFinding {
            kind: "wrapper_density".to_string(),
            severity: if health.wrapper_density_percent >= 50 {
                HealthSeverity::High
            } else {
                HealthSeverity::Medium
            },
            message: format!(
                "{} trivial wrappers ({}% of named functions)",
                health.trivial_wrappers, health.wrapper_density_percent
            ),
        });
    }
    push_threshold_finding(
        &mut findings,
        "duplicate_code",
        health.estimated_removable_lines,
        10,
        30,
        format!(
            "{} similarity group{}; about {} removable lines",
            health.duplicate_groups,
            if health.duplicate_groups == 1 { "" } else { "s" },
            health.estimated_removable_lines
        ),
    );
    push_threshold_finding(
        &mut findings,
        "branch_complexity",
        health.max_branch_complexity,
        8,
        15,
        format!("maximum branch complexity {}", health.max_branch_complexity),
    );
    push_threshold_finding(
        &mut findings,
        "control_nesting",
        health.max_control_nesting,
        3,
        5,
        format!("maximum control-flow nesting {}", health.max_control_nesting),
    );
    push_threshold_finding(
        &mut findings,
        "hook_sprawl",
        health.hook_calls,
        8,
        15,
        format!(
            "{} hook calls, including {} state and {} effect calls",
            health.hook_calls, health.state_calls, health.effect_calls
        ),
    );
    push_threshold_finding(
        &mut findings,
        "state_sprawl",
        health.state_calls,
        4,
        8,
        format!("{} state hook calls", health.state_calls),
    );
    push_threshold_finding(
        &mut findings,
        "effect_sprawl",
        health.effect_calls,
        3,
        5,
        format!("{} effect hook calls", health.effect_calls),
    );
    push_threshold_finding(
        &mut findings,
        "dependency_fan_out",
        health.dependencies,
        15,
        25,
        format!("{} imported dependencies", health.dependencies),
    );
    push_threshold_finding(
        &mut findings,
        "export_sprawl",
        health.exports,
        8,
        15,
        format!("{} exports", health.exports),
    );
    findings.sort_by(|left, right| {
        right.severity.cmp(&left.severity).then_with(|| left.kind.cmp(&right.kind))
    });
    findings
}

fn push_threshold_finding(
    findings: &mut Vec<HealthFinding>,
    kind: &str,
    value: usize,
    medium: usize,
    high: usize,
    message: String,
) {
    let severity = if value >= high {
        HealthSeverity::High
    } else if value >= medium {
        HealthSeverity::Medium
    } else {
        return;
    };
    findings.push(HealthFinding { kind: kind.to_string(), severity, message });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileEntry {
    name: String,
    functions: FunctionAnnotations,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeNode {
    name: String,
    rel_path: String,
    dirs: Vec<Self>,
    files: Vec<FileEntry>,
}

fn build_node(
    abs_path: &Path,
    project_root: &Path,
    all: bool,
    annotations: &BTreeMap<String, FunctionAnnotations>,
) -> Result<TreeNode> {
    let relative_path = rel_path(project_root, abs_path);
    let name = if relative_path.is_empty() {
        abs_path.file_name().map_or_else(|| ".".to_string(), |n| n.to_string_lossy().into_owned())
    } else {
        abs_path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };

    let mut dir_specs = Vec::new();
    let mut files = Vec::new();

    for entry in safe_readdir(abs_path)? {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        let entry_name = entry.file_name().to_string_lossy().into_owned();

        if file_type.is_dir() {
            if should_skip_dir(&entry_name, all) {
                continue;
            }
            dir_specs.push(entry.path());
            continue;
        }

        if file_type.is_file() && should_include_file(&entry_name) {
            let path = entry.path();
            let relative = rel_path(project_root, &path);
            files.push(FileEntry {
                name: entry_name,
                functions: annotations.get(&relative).cloned().unwrap_or_default(),
            });
        }
    }

    dir_specs.sort();
    files.sort_by(|a, b| natural_cmp(&a.name, &b.name));

    let dirs: Vec<TreeNode> = dir_specs
        .iter()
        .map(|path| build_node(path, project_root, all, annotations))
        .collect::<Result<_>>()?;

    Ok(TreeNode { name, rel_path: relative_path, dirs, files })
}

fn render_node(
    node: &TreeNode,
    lines: &mut Vec<String>,
    indent: &str,
    is_root: bool,
    branch_depth: usize,
    max_depth: usize,
    inline: usize,
) {
    let mut current = node;
    let mut chain_name = if is_root {
        if current.rel_path.is_empty() { String::new() } else { format!("{}/", current.rel_path) }
    } else {
        format!("{}/", current.name)
    };

    while !is_root && current.dirs.len() == 1 && current.files.is_empty() {
        current = &current.dirs[0];
        chain_name.push_str(&current.name);
        chain_name.push('/');
    }

    let is_branching = current.dirs.len() > 1
        || (!current.dirs.is_empty() && !current.files.is_empty())
        || current.files.len() > 1;
    let next_branch_depth =
        if is_root { branch_depth } else { branch_depth + usize::from(is_branching) };

    if !is_root && next_branch_depth >= max_depth {
        lines.push(format!("{indent}{chain_name} {}", summarize(current)));
        return;
    }

    if !is_root || !current.rel_path.is_empty() {
        if current.dirs.is_empty()
            && current.files.len() <= inline
            && current.files.iter().all(|file| file.functions == FunctionAnnotations::default())
        {
            let file_str = current.files.iter().map(fmt_file).collect::<Vec<_>>().join(", ");
            if file_str.is_empty() {
                lines.push(format!("{indent}{chain_name}"));
            } else {
                lines.push(format!("{indent}{chain_name}  {file_str}"));
            }
            return;
        }
        lines.push(format!("{indent}{chain_name}"));
    }

    let child_indent = if is_root { String::new() } else { format!("{indent}  ") };

    for dir in &current.dirs {
        render_node(dir, lines, &child_indent, false, next_branch_depth, max_depth, inline);
    }

    if !current.files.is_empty() {
        render_file_list(&current.files, lines, &child_indent, inline);
    }
}

fn render_file_list(files: &[FileEntry], lines: &mut Vec<String>, indent: &str, inline: usize) {
    if files.iter().all(|file| file.functions == FunctionAnnotations::default()) {
        for chunk in files.chunks(inline) {
            let rendered = chunk.iter().map(fmt_file).collect::<Vec<_>>().join(", ");
            lines.push(format!("{indent}{rendered}"));
        }
        return;
    }

    for file in files {
        let mut functions: Vec<String> = file
            .functions
            .labels
            .iter()
            .map(|function| {
                if function.markers.is_empty() {
                    function.name.clone()
                } else {
                    format!("{} {}", function.name, function.markers.join(""))
                }
            })
            .collect();
        if file.functions.anonymous_count > 0 {
            functions.push(format!("+{} anonymous", file.functions.anonymous_count));
        }

        if functions.is_empty() {
            lines.push(format!("{indent}{}", fmt_file(file)));
            continue;
        }
        for (index, chunk) in functions.chunks(inline).enumerate() {
            let prefix = if index == 0 { fmt_file(file) } else { " ".repeat(fmt_file(file).len()) };
            lines.push(format!("{indent}{prefix}  :: {}", chunk.join(", ")));
        }
    }
}

#[allow(clippy::too_many_lines)]
fn write_health_report(
    tree: &TreeNode,
    root: &str,
    only_findings: bool,
    top: Option<usize>,
    sort_by: HealthSort,
    json: bool,
) -> Result<()> {
    let mut files = Vec::new();
    collect_file_health(tree, &mut files);
    if only_findings {
        files.retain(|file| !file.findings.is_empty());
    }
    files.sort_by(|left, right| {
        let order = match sort_by {
            HealthSort::Severity => right
                .severity
                .cmp(&left.severity)
                .then_with(|| right.findings.len().cmp(&left.findings.len())),
            HealthSort::Coupling => right
                .capture_total
                .cmp(&left.capture_total)
                .then_with(|| right.capture_max.cmp(&left.capture_max)),
            HealthSort::Duplicates => right
                .estimated_removable_lines
                .cmp(&left.estimated_removable_lines)
                .then_with(|| right.duplicate_groups.cmp(&left.duplicate_groups)),
            HealthSort::Size => right.lines.cmp(&left.lines),
        };
        order.then_with(|| left.path.cmp(&right.path))
    });
    if let Some(limit) = top {
        files.truncate(limit);
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if json {
        serde_json::to_writer(
            &mut out,
            &HealthReport { version: 1, root: root.to_string(), files },
        )?;
        writeln!(out)?;
        return Ok(());
    }

    let high = files.iter().filter(|file| file.severity == HealthSeverity::High).count();
    let medium = files.iter().filter(|file| file.severity == HealthSeverity::Medium).count();
    writeln!(out, "# Project Health")?;
    writeln!(out)?;
    writeln!(out, "{} files shown; {high} high concern; {medium} medium concern", files.len())?;
    for file in files {
        writeln!(out)?;
        writeln!(out, "{} — {}", file.path, file.severity.label())?;
        writeln!(
            out,
            "├── size: {} lines; largest function: {} ({} lines)",
            file.lines,
            file.largest_function.as_deref().unwrap_or("none"),
            file.largest_function_lines
        )?;
        writeln!(
            out,
            "├── functions: {} named; {} anonymous callbacks; {} nested total; nesting maximum {}",
            file.named_functions,
            file.anonymous_callbacks,
            file.nested_functions,
            file.max_function_nesting
        )?;
        writeln!(
            out,
            "├── coupling: {} total parent-variable uses; maximum {}",
            file.capture_total, file.capture_max
        )?;
        writeln!(
            out,
            "├── wrappers: {} trivial ({}% of named functions)",
            file.trivial_wrappers, file.wrapper_density_percent
        )?;
        writeln!(
            out,
            "├── duplicates: {} group{}; {} function{}; about {} removable lines",
            file.duplicate_groups,
            if file.duplicate_groups == 1 { "" } else { "s" },
            file.duplicate_functions,
            if file.duplicate_functions == 1 { "" } else { "s" },
            file.estimated_removable_lines
        )?;
        writeln!(
            out,
            "├── complexity: branch maximum {}; control nesting maximum {}",
            file.max_branch_complexity, file.max_control_nesting
        )?;
        writeln!(
            out,
            "├── React: {} hooks; {} state calls; {} effect calls",
            file.hook_calls, file.state_calls, file.effect_calls
        )?;
        writeln!(
            out,
            "├── module: {} exports; {} imported dependencies",
            file.exports, file.dependencies
        )?;
        if file.findings.is_empty() {
            writeln!(out, "└── findings: none")?;
        } else {
            writeln!(out, "└── findings:")?;
            let finding_count = file.findings.len();
            for (index, finding) in file.findings.into_iter().enumerate() {
                let connector = if index + 1 == finding_count { "└──" } else { "├──" };
                writeln!(out, "    {connector} {}: {}", finding.severity.label(), finding.message)?;
            }
        }
    }
    Ok(())
}

fn collect_file_health(node: &TreeNode, output: &mut Vec<FileHealth>) {
    for file in &node.files {
        if let Some(health) = &file.functions.health {
            output.push(health.clone());
        }
    }
    for dir in &node.dirs {
        collect_file_health(dir, output);
    }
}

fn render_detailed_tree(
    node: &TreeNode,
    lines: &mut Vec<String>,
    max_depth: usize,
    min_lines: u32,
    max_lines: u32,
) {
    if !node.rel_path.is_empty() {
        lines.push(format!("{}/", node.rel_path));
    }
    render_detailed_node(node, lines, "", 0, max_depth, min_lines, max_lines);
    if !node_has_detailed_matches(node, min_lines, max_lines) {
        lines.push(format!("(no named functions between {min_lines} and {max_lines} lines)"));
    }
}

fn render_detailed_node(
    node: &TreeNode,
    lines: &mut Vec<String>,
    indent: &str,
    depth: usize,
    max_depth: usize,
    min_lines: u32,
    max_lines: u32,
) {
    let dirs: Vec<_> = node
        .dirs
        .iter()
        .filter(|dir| node_has_detailed_matches(dir, min_lines, max_lines))
        .collect();
    let files: Vec<_> = node
        .files
        .iter()
        .filter(|file| !detailed_labels(file, min_lines, max_lines).is_empty())
        .collect();
    let child_count = dirs.len() + files.len();

    for (index, dir) in dirs.iter().enumerate() {
        let is_last = index + 1 == child_count;
        let connector = if is_last { "└──" } else { "├──" };
        if depth + 1 >= max_depth {
            lines.push(format!(
                "{indent}{connector} {}/ ({} matching files)",
                dir.name,
                detailed_file_count(dir, min_lines, max_lines)
            ));
            continue;
        }
        lines.push(format!("{indent}{connector} {}/", dir.name));
        let child_indent = format!("{indent}{}   ", if is_last { " " } else { "│" });
        render_detailed_node(dir, lines, &child_indent, depth + 1, max_depth, min_lines, max_lines);
    }

    for (file_index, file) in files.iter().enumerate() {
        let index = dirs.len() + file_index;
        let is_last = index + 1 == child_count;
        let connector = if is_last { "└──" } else { "├──" };
        lines.push(format!("{indent}{connector} {}", file.name));
        let child_indent = format!("{indent}{}   ", if is_last { " " } else { "│" });
        let labels = detailed_labels(file, min_lines, max_lines);
        render_detailed_functions(&labels, lines, &child_indent);
    }
}

fn node_has_detailed_matches(node: &TreeNode, min_lines: u32, max_lines: u32) -> bool {
    node.files.iter().any(|file| !detailed_labels(file, min_lines, max_lines).is_empty())
        || node.dirs.iter().any(|dir| node_has_detailed_matches(dir, min_lines, max_lines))
}

fn detailed_file_count(node: &TreeNode, min_lines: u32, max_lines: u32) -> usize {
    node.files.iter().filter(|file| !detailed_labels(file, min_lines, max_lines).is_empty()).count()
        + node.dirs.iter().map(|dir| detailed_file_count(dir, min_lines, max_lines)).sum::<usize>()
}

fn detailed_labels(file: &FileEntry, min_lines: u32, max_lines: u32) -> Vec<&FunctionLabel> {
    let by_name: BTreeMap<_, _> =
        file.functions.labels.iter().map(|label| (label.name.as_str(), label)).collect();
    let mut visible = BTreeSet::new();
    for label in &file.functions.labels {
        if !(min_lines..=max_lines).contains(&label.lines) {
            continue;
        }
        visible.insert(label.name.clone());
        let mut parent = label.parent.as_deref();
        while let Some(name) = parent {
            if !visible.insert(name.to_string()) {
                break;
            }
            parent = by_name.get(name).and_then(|label| label.parent.as_deref());
        }
    }
    let mut labels: Vec<_> =
        file.functions.labels.iter().filter(|label| visible.contains(&label.name)).collect();
    labels
        .sort_by(|left, right| left.line.cmp(&right.line).then_with(|| left.name.cmp(&right.name)));
    labels
}

fn render_detailed_functions(labels: &[&FunctionLabel], lines: &mut Vec<String>, indent: &str) {
    let visible: BTreeSet<_> = labels.iter().map(|label| label.name.as_str()).collect();
    let mut children: BTreeMap<Option<&str>, Vec<&FunctionLabel>> = BTreeMap::new();
    for label in labels {
        let parent = label.parent.as_deref().filter(|parent| visible.contains(parent));
        children.entry(parent).or_default().push(label);
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|label| (label.line, label.name.as_str()));
    }
    if let Some(roots) = children.get(&None) {
        for (index, root) in roots.iter().enumerate() {
            render_detailed_function(root, &children, lines, indent, index + 1 == roots.len());
        }
    }
}

fn render_detailed_function(
    label: &FunctionLabel,
    children: &BTreeMap<Option<&str>, Vec<&FunctionLabel>>,
    lines: &mut Vec<String>,
    indent: &str,
    is_last: bool,
) {
    let connector = if is_last { "└──" } else { "├──" };
    lines.push(format!("{indent}{connector} {}", detailed_function_description(label)));
    let child_indent = format!("{indent}{}   ", if is_last { " " } else { "│" });
    if let Some(nested) = children.get(&Some(label.name.as_str())) {
        for (index, child) in nested.iter().enumerate() {
            render_detailed_function(
                child,
                children,
                lines,
                &child_indent,
                index + 1 == nested.len(),
            );
        }
    }
}

fn detailed_function_description(label: &FunctionLabel) -> String {
    let short_name = label.name.rsplit('.').next().unwrap_or(&label.name);
    let mut details = vec![format!("{} lines", label.lines)];
    if label.parent.is_some() {
        if label.capture_count == 0 {
            details.push("uses no parent variables".to_string());
        } else {
            details.push(format!(
                "uses {} parent variable{}",
                label.capture_count,
                if label.capture_count == 1 { "" } else { "s" }
            ));
        }
    }
    if let Some(reason) = label.low_value_reason.as_deref() {
        details.push(low_value_explanation(reason).to_string());
    }
    if label.hoistable {
        details.push("can move outside its parent".to_string());
    }
    if !label.duplicate_peers.is_empty() {
        let mut peers: Vec<_> = label
            .duplicate_peers
            .iter()
            .take(2)
            .map(|peer| format!("{} in {}:{}", peer.name, peer.path, peer.line))
            .collect();
        if label.duplicate_peers.len() > 2 {
            peers.push(format!("{} more", label.duplicate_peers.len() - 2));
        }
        details.push(format!("similar to {}", peers.join(", ")));
    }
    format!("{short_name} — {}", details.join("; "))
}

fn low_value_explanation(reason: &str) -> &str {
    match reason {
        "empty" | "empty_return" => "empty function; review whether it is needed",
        "constant_return" => "only returns a constant; review for inlining",
        "identity_return" => "only returns its input; review whether it adds value",
        "property_return" => "only returns one property; review for inlining",
        "thin_wrapper" => "only returns another call; review for inlining",
        "side_effect_wrapper" => "only forwards a call; review for inlining",
        _ => "small trivial wrapper; review whether it adds value",
    }
}

fn fmt_file(file: &FileEntry) -> String {
    strip_known_ext(&file.name)
}

fn summarize(node: &TreeNode) -> String {
    let (dirs, files) = count_tree(node);
    let mut parts = Vec::new();
    if dirs > 0 {
        parts.push(format!("{dirs}d"));
    }
    if files > 0 {
        parts.push(format!("{files}f"));
    }
    format!("({})", parts.join(" "))
}

fn count_tree(node: &TreeNode) -> (usize, usize) {
    let mut dirs = node.dirs.len();
    let mut files = node.files.len();
    for dir in &node.dirs {
        let (sub_dirs, sub_files) = count_tree(dir);
        dirs += sub_dirs;
        files += sub_files;
    }
    (dirs, files)
}

fn should_skip_dir(name: &str, all: bool) -> bool {
    if IGNORE_DIRS.contains(&name) || name.starts_with('.') {
        return true;
    }
    !all && IGNORE_TEST_DIRS.contains(&name)
}

fn should_include_file(name: &str) -> bool {
    if INCLUDE_FILE_NAMES.contains(&name) {
        return true;
    }

    let ext = name.rsplit('.').next().unwrap_or("");
    INCLUDE_EXTS.contains(&ext)
}

fn strip_known_ext(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("");
    if STRIP_EXTS.contains(&ext) && name.contains('.') {
        let suffix = format!(".{ext}");
        return name.strip_suffix(&suffix).unwrap_or(name).to_string();
    }
    name.to_string()
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().trim_matches('/').to_string())
        .unwrap_or_default()
}

fn safe_readdir(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("Cannot read {}", dir.display()))?.flatten()
    {
        entries.push(entry);
    }
    Ok(entries)
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let left_parts = split_natural(left);
    let right_parts = split_natural(right);

    for (l, r) in left_parts.iter().zip(&right_parts) {
        let ord = match (l.parse::<usize>(), r.parse::<usize>()) {
            (Ok(ln), Ok(rn)) => ln.cmp(&rn),
            _ => l.cmp(r),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }

    left_parts.len().cmp(&right_parts.len()).then_with(|| left.cmp(right))
}

fn split_natural(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut current_is_digit = None;

    for ch in value.chars() {
        let is_digit = ch.is_ascii_digit();
        if current_is_digit.is_some_and(|flag| flag != is_digit) {
            parts.push(current);
            current = String::new();
        }
        current.push(ch.to_ascii_lowercase());
        current_is_digit = Some(is_digit);
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn function_annotations_include_layered_markers() {
        let tmp = tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("a.ts"),
            r"
function outer() {
  const local = 1;
  function hoistable() { return 1; }
  const captured = () => local;
}
function tiny() { return save(); }
function duplicateOne(values: number[]) {
  let total = 0;
  for (const value of values) {
    if (value > 0) {
      total += value;
    }
  }


  return total;
}
",
        )
        .expect("write a");
        fs::write(
            tmp.path().join("b.ts"),
            r"
function duplicateOne(values: number[]) {
  let total = 0;
  for (const value of values) {
    if (value > 0) {
      total += value;
    }
  }


  return total;
}
",
        )
        .expect("write b");
        let root = tmp.path().canonicalize().expect("canonical root");
        let annotations = build_function_annotations(&root, &root, false, 3, 0.0, false, false)
            .expect("annotations");
        let a = annotations.get("a.ts").expect("a annotations");
        let marker_map: BTreeMap<_, _> =
            a.labels.iter().map(|label| (label.name.as_str(), label.markers.as_slice())).collect();

        assert!(marker_map["outer.hoistable"].contains(&"[H]".to_string()));
        assert!(marker_map["outer.hoistable"].contains(&"[L]".to_string()));
        assert!(marker_map["outer.captured"].contains(&"[C:1]".to_string()));
        assert!(
            marker_map["duplicateOne"].contains(&"[D:2]".to_string()),
            "annotations: {annotations:?}"
        );
    }

    #[test]
    fn detailed_tree_renders_hierarchy_lines_and_similarity_in_plain_language() {
        let tmp = tempdir().expect("tempdir");
        for (file, component) in [("a.tsx", "Card"), ("b.tsx", "OtherCard")] {
            fs::write(
                tmp.path().join(file),
                format!(
                    "function {component}() {{\n  const setRejected = (value: boolean) => value;\n  function flash() {{\n    setRejected(true);\n  }}\n  return null;\n}}\n"
                ),
            )
            .expect("write component");
        }
        let root = tmp.path().canonicalize().expect("canonical root");
        let annotations = build_function_annotations(&root, &root, false, 3, 0.87, false, false)
            .expect("annotations");
        let tree = build_node(&root, &root, false, &annotations).expect("tree");
        let mut lines = Vec::new();
        render_detailed_tree(&tree, &mut lines, 6, 3, 3);
        let output = lines.join("\n");

        assert!(output.contains("a.tsx"), "output: {output}");
        assert!(output.contains("Card — 7 lines"), "output: {output}");
        assert!(output.contains("flash — 3 lines; uses 1 parent variable"), "output: {output}");
        assert!(output.contains("similar to OtherCard.flash in b.tsx:3"), "output: {output}");
        assert!(!output.contains("[C:"), "output: {output}");
        assert!(!output.contains("[D:"), "output: {output}");
    }

    #[test]
    fn health_metrics_include_size_react_module_and_findings() {
        let tmp = tempdir().expect("tempdir");
        let mut source = "import { useState } from 'react';\nexport function LargeComponent() {\n  const [value] = useState(0);\n".to_string();
        for _ in 0..300 {
            source.push_str("  // padding\n");
        }
        source.push_str("  return value;\n}\n");
        fs::write(tmp.path().join("large.tsx"), source).expect("write source");
        let root = tmp.path().canonicalize().expect("canonical root");
        let annotations = build_function_annotations(&root, &root, false, 3, 0.87, true, false)
            .expect("annotations");
        let health = annotations["large.tsx"].health.as_ref().expect("health");

        assert!(health.lines >= 305);
        assert_eq!(health.hook_calls, 1);
        assert_eq!(health.state_calls, 1);
        assert_eq!(health.dependencies, 1);
        assert!(health.findings.iter().any(|finding| finding.kind == "oversized_file"));
        let json = serde_json::to_value(health).expect("health json");
        assert_eq!(json["stateCalls"], 1);
    }

    #[test]
    fn includes_common_text_files() {
        assert!(should_include_file("main.rs"));
        assert!(should_include_file("README.md"));
        assert!(should_include_file("Cargo.toml"));
        assert!(should_include_file("Dockerfile"));
        assert!(!should_include_file("image.png"));
    }

    #[test]
    fn strips_known_extensions() {
        assert_eq!(strip_known_ext("main.rs"), "main");
        assert_eq!(strip_known_ext("index.test.ts"), "index.test");
        assert_eq!(strip_known_ext("Cargo.toml"), "Cargo.toml");
        assert_eq!(strip_known_ext("Dockerfile"), "Dockerfile");
    }

    #[test]
    fn natural_sort_handles_numbers() {
        let mut names = [
            FileEntry { name: "file10.ts".to_string(), functions: FunctionAnnotations::default() },
            FileEntry { name: "file2.ts".to_string(), functions: FunctionAnnotations::default() },
            FileEntry { name: "file1.ts".to_string(), functions: FunctionAnnotations::default() },
        ];
        names.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        let ordered: Vec<_> = names.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(ordered, vec!["file1.ts", "file2.ts", "file10.ts"]);
    }

    #[test]
    fn tree_skips_ignored_and_test_dirs_by_default() {
        let tmp = tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("src")).expect("src dir");
        fs::create_dir_all(tmp.path().join("tests")).expect("tests dir");
        fs::create_dir_all(tmp.path().join("target")).expect("target dir");
        fs::write(tmp.path().join("src/main.rs"), "fn main() {}").expect("write main");
        fs::write(tmp.path().join("tests/main_test.rs"), "").expect("write test");
        fs::write(tmp.path().join("target/cache.txt"), "").expect("write cache");

        let node = build_node(tmp.path(), tmp.path(), false, &BTreeMap::new()).expect("build tree");
        let dir_names: BTreeSet<_> = node.dirs.iter().map(|dir| dir.name.as_str()).collect();
        assert!(dir_names.contains("src"));
        assert!(!dir_names.contains("tests"));
        assert!(!dir_names.contains("target"));
    }

    #[test]
    fn tree_includes_test_dirs_with_all_flag() {
        let tmp = tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("tests")).expect("tests dir");
        fs::write(tmp.path().join("tests/main_test.rs"), "").expect("write test");

        let node = build_node(tmp.path(), tmp.path(), true, &BTreeMap::new()).expect("build tree");
        let dir_names: BTreeSet<_> = node.dirs.iter().map(|dir| dir.name.as_str()).collect();
        assert!(dir_names.contains("tests"));
    }

    // ── render_node ──────────────────────────────────────────────

    #[test]
    fn render_node_shows_nested_dirs_and_files() {
        let node = TreeNode {
            name: "root".into(),
            rel_path: String::new(),
            dirs: vec![TreeNode {
                name: "src".into(),
                rel_path: "src".into(),
                dirs: vec![TreeNode {
                    name: "utils".into(),
                    rel_path: "src/utils".into(),
                    dirs: vec![],
                    files: vec![FileEntry {
                        name: "helper.ts".into(),
                        functions: FunctionAnnotations::default(),
                    }],
                }],
                files: vec![
                    FileEntry { name: "main.rs".into(), functions: FunctionAnnotations::default() },
                    FileEntry { name: "lib.rs".into(), functions: FunctionAnnotations::default() },
                ],
            }],
            files: vec![FileEntry {
                name: "Cargo.toml".into(),
                functions: FunctionAnnotations::default(),
            }],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", true, 0, 10, 3);
        let text = lines.join("\n");

        assert!(text.contains("src/"), "should contain dir 'src/'");
        assert!(text.contains("utils/"), "should contain dir 'utils/'");
        assert!(text.contains("main"), "should contain file 'main'");
        assert!(text.contains("lib"), "should contain file 'lib'");
        assert!(text.contains("helper"), "should contain file 'helper'");
        assert!(text.contains("Cargo.toml"), "should contain file 'Cargo.toml'");
    }

    #[test]
    fn render_node_root_with_rel_path_shows_prefix() {
        let node = TreeNode {
            name: "sub".into(),
            rel_path: "sub".into(),
            dirs: vec![],
            files: vec![FileEntry {
                name: "index.ts".into(),
                functions: FunctionAnnotations::default(),
            }],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", true, 0, 10, 3);
        assert!(lines.iter().any(|l| l.contains("sub/")), "root with rel_path should show 'sub/'");
    }

    // ── summarize ────────────────────────────────────────────────

    #[test]
    fn summarize_dirs_and_files() {
        let node = TreeNode {
            name: "pkg".into(),
            rel_path: "pkg".into(),
            dirs: vec![
                TreeNode {
                    name: "a".into(),
                    rel_path: "pkg/a".into(),
                    dirs: vec![],
                    files: vec![FileEntry {
                        name: "x.ts".into(),
                        functions: FunctionAnnotations::default(),
                    }],
                },
                TreeNode {
                    name: "b".into(),
                    rel_path: "pkg/b".into(),
                    dirs: vec![],
                    files: vec![FileEntry {
                        name: "y.ts".into(),
                        functions: FunctionAnnotations::default(),
                    }],
                },
            ],
            files: vec![
                FileEntry { name: "one.rs".into(), functions: FunctionAnnotations::default() },
                FileEntry { name: "two.rs".into(), functions: FunctionAnnotations::default() },
                FileEntry { name: "three.rs".into(), functions: FunctionAnnotations::default() },
            ],
        };

        assert_eq!(summarize(&node), "(2d 5f)");
    }

    #[test]
    fn summarize_only_dirs() {
        let node = TreeNode {
            name: "d".into(),
            rel_path: "d".into(),
            dirs: vec![TreeNode {
                name: "inner".into(),
                rel_path: "d/inner".into(),
                dirs: vec![],
                files: vec![],
            }],
            files: vec![],
        };
        assert_eq!(summarize(&node), "(1d)");
    }

    #[test]
    fn summarize_only_files() {
        let node = TreeNode {
            name: "d".into(),
            rel_path: "d".into(),
            dirs: vec![],
            files: vec![FileEntry {
                name: "a.rs".into(),
                functions: FunctionAnnotations::default(),
            }],
        };
        assert_eq!(summarize(&node), "(1f)");
    }

    #[test]
    fn summarize_empty() {
        let node = TreeNode { name: "d".into(), rel_path: "d".into(), dirs: vec![], files: vec![] };
        assert_eq!(summarize(&node), "()");
    }

    // ── count_tree ───────────────────────────────────────────────

    #[test]
    fn count_tree_recursive() {
        let node = TreeNode {
            name: "root".into(),
            rel_path: String::new(),
            dirs: vec![
                TreeNode {
                    name: "a".into(),
                    rel_path: "a".into(),
                    dirs: vec![TreeNode {
                        name: "a1".into(),
                        rel_path: "a/a1".into(),
                        dirs: vec![],
                        files: vec![
                            FileEntry {
                                name: "f1.rs".into(),
                                functions: FunctionAnnotations::default(),
                            },
                            FileEntry {
                                name: "f2.rs".into(),
                                functions: FunctionAnnotations::default(),
                            },
                        ],
                    }],
                    files: vec![FileEntry {
                        name: "f3.rs".into(),
                        functions: FunctionAnnotations::default(),
                    }],
                },
                TreeNode {
                    name: "b".into(),
                    rel_path: "b".into(),
                    dirs: vec![],
                    files: vec![FileEntry {
                        name: "f4.rs".into(),
                        functions: FunctionAnnotations::default(),
                    }],
                },
            ],
            files: vec![FileEntry {
                name: "f5.rs".into(),
                functions: FunctionAnnotations::default(),
            }],
        };

        let (dirs, files) = count_tree(&node);
        // dirs: a, a1, b = 3
        assert_eq!(dirs, 3);
        // files: f1, f2, f3, f4, f5 = 5
        assert_eq!(files, 5);
    }

    #[test]
    fn count_tree_leaf() {
        let node = TreeNode {
            name: "leaf".into(),
            rel_path: "leaf".into(),
            dirs: vec![],
            files: vec![FileEntry {
                name: "only.rs".into(),
                functions: FunctionAnnotations::default(),
            }],
        };
        assert_eq!(count_tree(&node), (0, 1));
    }

    // ── should_skip_dir ──────────────────────────────────────────

    #[test]
    fn should_skip_all_ignore_dirs() {
        for dir in IGNORE_DIRS {
            assert!(should_skip_dir(dir, false), "{dir} should be skipped");
            assert!(should_skip_dir(dir, true), "{dir} should be skipped even with all=true");
        }
    }

    #[test]
    fn should_skip_hidden_dirs() {
        assert!(should_skip_dir(".hidden", false));
        assert!(should_skip_dir(".config", true));
        assert!(should_skip_dir(".secret", false));
    }

    #[test]
    fn should_skip_test_dirs_without_all() {
        for dir in IGNORE_TEST_DIRS {
            assert!(should_skip_dir(dir, false), "{dir} should be skipped with all=false");
        }
    }

    #[test]
    fn should_not_skip_test_dirs_with_all() {
        for dir in IGNORE_TEST_DIRS {
            assert!(!should_skip_dir(dir, true), "{dir} should NOT be skipped with all=true");
        }
    }

    #[test]
    fn should_not_skip_normal_dirs() {
        assert!(!should_skip_dir("src", false));
        assert!(!should_skip_dir("lib", false));
        assert!(!should_skip_dir("packages", true));
    }

    // ── should_include_file ──────────────────────────────────────

    #[test]
    fn should_include_all_known_extensions() {
        for ext in INCLUDE_EXTS {
            let name = format!("file.{ext}");
            assert!(should_include_file(&name), "{name} should be included");
        }
    }

    #[test]
    fn should_include_special_file_names() {
        for name in INCLUDE_FILE_NAMES {
            assert!(should_include_file(name), "{name} should be included");
        }
    }

    #[test]
    fn should_exclude_unknown_extensions() {
        assert!(!should_include_file("image.png"));
        assert!(!should_include_file("video.mp4"));
        assert!(!should_include_file("archive.zip"));
        assert!(!should_include_file("binary.exe"));
        assert!(!should_include_file("font.woff2"));
    }

    // ── strip_known_ext ──────────────────────────────────────────

    #[test]
    fn strip_ext_multiple_dots() {
        assert_eq!(strip_known_ext("my.component.test.tsx"), "my.component.test");
        assert_eq!(strip_known_ext("a.b.c.d.js"), "a.b.c.d");
    }

    #[test]
    fn strip_ext_no_extension() {
        assert_eq!(strip_known_ext("Makefile"), "Makefile");
        assert_eq!(strip_known_ext("LICENSE"), "LICENSE");
    }

    #[test]
    fn strip_ext_non_strip_extensions_kept() {
        assert_eq!(strip_known_ext("styles.css"), "styles.css");
        assert_eq!(strip_known_ext("config.json"), "config.json");
        assert_eq!(strip_known_ext("readme.md"), "readme.md");
        assert_eq!(strip_known_ext("schema.sql"), "schema.sql");
        assert_eq!(strip_known_ext("config.yaml"), "config.yaml");
    }

    #[test]
    fn strip_ext_all_strip_exts() {
        for ext in STRIP_EXTS {
            let name = format!("file.{ext}");
            assert_eq!(strip_known_ext(&name), "file", "should strip .{ext}");
        }
    }

    // ── natural_cmp ──────────────────────────────────────────────

    #[test]
    fn natural_cmp_equal_strings() {
        assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
        assert_eq!(natural_cmp("file1.ts", "file1.ts"), Ordering::Equal);
    }

    #[test]
    fn natural_cmp_multiple_number_segments() {
        assert_eq!(natural_cmp("v1.2.3", "v1.2.10"), Ordering::Less);
        assert_eq!(natural_cmp("v1.10.1", "v1.2.1"), Ordering::Greater);
    }

    #[test]
    fn natural_cmp_pure_numbers() {
        assert_eq!(natural_cmp("9", "10"), Ordering::Less);
        assert_eq!(natural_cmp("100", "20"), Ordering::Greater);
        assert_eq!(natural_cmp("42", "42"), Ordering::Equal);
    }

    #[test]
    fn natural_cmp_case_insensitive_sorting() {
        // split_natural lowercases parts, so "Abc" and "abc" have equal segments,
        // but the final tiebreaker uses original strings: "Abc" < "abc" in ASCII
        assert_eq!(natural_cmp("Abc", "abc"), Ordering::Less);
        // Same-case strings are truly equal
        assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
        // Lowercased parts sort together: "File" and "file" are adjacent
        assert_eq!(natural_cmp("File1", "file2"), Ordering::Less);
    }

    #[test]
    fn natural_cmp_prefix() {
        assert_eq!(natural_cmp("file", "file1"), Ordering::Less);
        assert_eq!(natural_cmp("file1", "file"), Ordering::Greater);
    }

    // ── chain collapsing in render_node ──────────────────────────

    #[test]
    fn render_node_collapses_single_child_chain() {
        // a/ -> b/ -> c/ with files only in c
        let node = TreeNode {
            name: "a".into(),
            rel_path: "a".into(),
            dirs: vec![TreeNode {
                name: "b".into(),
                rel_path: "a/b".into(),
                dirs: vec![TreeNode {
                    name: "c".into(),
                    rel_path: "a/b/c".into(),
                    dirs: vec![],
                    files: vec![FileEntry {
                        name: "leaf.ts".into(),
                        functions: FunctionAnnotations::default(),
                    }],
                }],
                files: vec![],
            }],
            files: vec![],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", false, 0, 10, 3);
        let text = lines.join("\n");
        assert!(text.contains("a/b/c/"), "chain should be collapsed to 'a/b/c/'");
    }

    #[test]
    fn render_node_no_collapse_when_multiple_children() {
        let node = TreeNode {
            name: "a".into(),
            rel_path: "a".into(),
            dirs: vec![
                TreeNode {
                    name: "b".into(),
                    rel_path: "a/b".into(),
                    dirs: vec![],
                    files: vec![FileEntry {
                        name: "x.ts".into(),
                        functions: FunctionAnnotations::default(),
                    }],
                },
                TreeNode {
                    name: "c".into(),
                    rel_path: "a/c".into(),
                    dirs: vec![],
                    files: vec![FileEntry {
                        name: "y.ts".into(),
                        functions: FunctionAnnotations::default(),
                    }],
                },
            ],
            files: vec![],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", false, 0, 10, 3);
        let text = lines.join("\n");
        // Should NOT collapse: a has 2 children
        assert!(text.contains("a/\n"), "a/ should be its own line");
        assert!(text.contains("b/"), "child b/ should appear");
        assert!(text.contains("c/"), "child c/ should appear");
    }

    #[test]
    fn render_node_no_collapse_when_parent_has_files() {
        let node = TreeNode {
            name: "a".into(),
            rel_path: "a".into(),
            dirs: vec![TreeNode {
                name: "b".into(),
                rel_path: "a/b".into(),
                dirs: vec![],
                files: vec![FileEntry {
                    name: "inner.ts".into(),
                    functions: FunctionAnnotations::default(),
                }],
            }],
            files: vec![FileEntry {
                name: "outer.rs".into(),
                functions: FunctionAnnotations::default(),
            }],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", false, 0, 10, 3);
        let text = lines.join("\n");
        // a has files, so it should NOT collapse with b
        assert!(!text.contains("a/b/"), "should not collapse when parent has files");
    }

    // ── render_node depth truncation ─────────────────────────────

    #[test]
    fn render_node_truncates_at_max_depth() {
        let node = TreeNode {
            name: "top".into(),
            rel_path: "top".into(),
            dirs: vec![TreeNode {
                name: "mid".into(),
                rel_path: "top/mid".into(),
                dirs: vec![TreeNode {
                    name: "deep".into(),
                    rel_path: "top/mid/deep".into(),
                    dirs: vec![],
                    files: vec![
                        FileEntry {
                            name: "a.rs".into(),
                            functions: FunctionAnnotations::default(),
                        },
                        FileEntry {
                            name: "b.rs".into(),
                            functions: FunctionAnnotations::default(),
                        },
                    ],
                }],
                files: vec![
                    FileEntry { name: "c.rs".into(), functions: FunctionAnnotations::default() },
                    FileEntry { name: "d.rs".into(), functions: FunctionAnnotations::default() },
                ],
            }],
            files: vec![],
        };

        let mut lines = Vec::new();
        // max_depth=1 should truncate at first branching level
        render_node(&node, &mut lines, "", false, 0, 1, 3);
        let text = lines.join("\n");
        // Should show a summary rather than expanding everything
        assert!(text.contains('d') || text.contains('f'), "truncated node should show summary");
    }

    // ── rel_path ─────────────────────────────────────────────────

    #[test]
    fn rel_path_matching_prefix() {
        let root = Path::new("/home/user/project");
        let child = Path::new("/home/user/project/src/main.rs");
        assert_eq!(rel_path(root, child), "src/main.rs");
    }

    #[test]
    fn rel_path_same_path() {
        let root = Path::new("/home/user/project");
        assert_eq!(rel_path(root, root), "");
    }

    #[test]
    fn rel_path_non_matching_prefix() {
        let root = Path::new("/home/user/project");
        let other = Path::new("/tmp/other");
        // strip_prefix fails, returns empty string
        assert_eq!(rel_path(root, other), "");
    }

    // ── build_node with filesystem ───────────────────────────────

    #[test]
    fn build_node_filters_files_by_extension() {
        let tmp = tempdir().expect("tempdir");
        fs::write(tmp.path().join("app.ts"), "").expect("write ts");
        fs::write(tmp.path().join("style.css"), "").expect("write css");
        fs::write(tmp.path().join("image.png"), "").expect("write png");
        fs::write(tmp.path().join("data.bin"), "").expect("write bin");

        let node = build_node(tmp.path(), tmp.path(), false, &BTreeMap::new()).expect("build");
        let file_names: BTreeSet<_> = node.files.iter().map(|f| f.name.as_str()).collect();
        assert!(file_names.contains("app.ts"));
        assert!(file_names.contains("style.css"));
        assert!(!file_names.contains("image.png"));
        assert!(!file_names.contains("data.bin"));
    }

    #[test]
    fn build_node_includes_special_file_names() {
        let tmp = tempdir().expect("tempdir");
        fs::write(tmp.path().join("Dockerfile"), "FROM rust").expect("write");
        fs::write(tmp.path().join("Makefile"), "all:").expect("write");
        fs::write(tmp.path().join("randomfile"), "stuff").expect("write");

        let node = build_node(tmp.path(), tmp.path(), false, &BTreeMap::new()).expect("build");
        let file_names: BTreeSet<_> = node.files.iter().map(|f| f.name.as_str()).collect();
        assert!(file_names.contains("Dockerfile"));
        assert!(file_names.contains("Makefile"));
        assert!(!file_names.contains("randomfile"));
    }

    #[test]
    fn build_node_skips_hidden_directories() {
        let tmp = tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join(".hidden")).expect("mkdir");
        fs::write(tmp.path().join(".hidden/secret.rs"), "").expect("write");
        fs::create_dir_all(tmp.path().join("visible")).expect("mkdir");
        fs::write(tmp.path().join("visible/code.rs"), "").expect("write");

        let node = build_node(tmp.path(), tmp.path(), false, &BTreeMap::new()).expect("build");
        let dir_names: BTreeSet<_> = node.dirs.iter().map(|d| d.name.as_str()).collect();
        assert!(!dir_names.contains(".hidden"));
        assert!(dir_names.contains("visible"));
    }
}
