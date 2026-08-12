use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value};

use crate::scan::types::FunctionRole;
use crate::slop::test_detectors::duplicated_test_groups;
use crate::slop::types::{
    DeclarationFact, DeclarationKind, DiffSummary, ProjectFacts, SlopConfidence, SlopEvidence,
    SlopFinding, SlopKind, SourceSpan, SymbolKey,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffFileStatus {
    Added,
    Modified,
    Renamed { old_path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFileScope {
    pub path: String,
    pub status: DiffFileStatus,
    pub added: Vec<LineRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DependencyAddition {
    pub manifest: String,
    pub section: String,
    pub name: String,
    pub requirement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffScope {
    pub requested_base: String,
    pub resolved_base: String,
    pub files: BTreeMap<String, DiffFileScope>,
    pub dependency_additions: Vec<DependencyAddition>,
}

impl DiffScope {
    pub fn intersects(&self, span: &SourceSpan) -> bool {
        self.files.get(&span.path).is_some_and(|file| {
            file.added
                .iter()
                .any(|range| range.start <= span.end_line && range.end >= span.start_line)
        })
    }

    pub fn fully_contains(&self, span: &SourceSpan) -> bool {
        self.files.get(&span.path).is_some_and(|file| {
            file.added
                .iter()
                .any(|range| range.start <= span.start_line && range.end >= span.end_line)
        })
    }

    pub fn contains_line(&self, path: &str, line: u32) -> bool {
        self.files.get(path).is_some_and(|file| {
            file.added.iter().any(|range| range.start <= line && line <= range.end)
        })
    }

    pub fn added_line_count(&self) -> usize {
        self.files
            .values()
            .flat_map(|file| &file.added)
            .map(|range| (range.end - range.start + 1) as usize)
            .sum()
    }

    pub fn summary(&self) -> DiffSummary {
        DiffSummary {
            requested_base: self.requested_base.clone(),
            resolved_base: self.resolved_base.clone(),
            changed_files: self.files.len(),
            added_lines: self.added_line_count(),
        }
    }
}

pub fn load_diff_scope(root: &Path, base: &str) -> Result<DiffScope> {
    let root = root.canonicalize().context("Cannot resolve project root for --base")?;
    require_worktree(&root)?;
    let repo_root_output = run_git(&root, ["rev-parse", "--show-toplevel"], "locate Git worktree")?;
    let repo_root_text = utf8_stdout(&repo_root_output, "Git worktree path")?;
    let repo_root = PathBuf::from(repo_root_text.trim())
        .canonicalize()
        .with_context(|| format!("cannot resolve Git worktree root `{}`", repo_root_text.trim()))?;
    let root_prefix = root
        .strip_prefix(&repo_root)
        .map_err(|_| anyhow!("analysis root is not inside the resolved Git worktree"))?;

    let revision = format!("{base}^{{commit}}");
    let resolved_output = run_git(
        &root,
        ["rev-parse", "--verify", "--end-of-options", revision.as_str()],
        &format!("resolve --base `{base}`"),
    )?;
    let resolved_base = utf8_stdout(&resolved_output, "resolved Git base")?.trim().to_string();

    let names = run_git(
        &root,
        [
            "diff",
            "--relative",
            "--name-status",
            "-z",
            "--find-renames",
            resolved_base.as_str(),
            "--",
            ".",
        ],
        "enumerate Git diff files",
    )?;
    let statuses = parse_name_status(&names.stdout)?;
    let mut files = BTreeMap::new();
    for (path, status) in statuses {
        let mut arguments = vec![
            "diff".to_string(),
            "--relative".to_string(),
            "--unified=0".to_string(),
            "--no-color".to_string(),
            "--no-ext-diff".to_string(),
            "--find-renames".to_string(),
            resolved_base.clone(),
            "--".to_string(),
        ];
        if let DiffFileStatus::Renamed { old_path } = &status {
            arguments.push(format!(":(literal){old_path}"));
        }
        arguments.push(format!(":(literal){path}"));
        let patch = run_git_owned(&root, &arguments, &format!("load Git patch for `{path}`"))?;
        let added = parse_added_ranges(&patch.stdout)?;
        files.insert(path.clone(), DiffFileScope { path, status, added });
    }

    let dependency_additions =
        load_dependency_additions(&root, &repo_root, root_prefix, &resolved_base, &files)?;
    Ok(DiffScope { requested_base: base.to_string(), resolved_base, files, dependency_additions })
}

fn require_worktree(root: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("--no-pager")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .context("failed to execute Git for --base")?;
    if !output.status.success() || output.stdout != b"true\n" {
        bail!("--base requires a Git worktree at `{}`", root.display());
    }
    Ok(())
}

fn run_git<'a>(
    root: &Path,
    args: impl IntoIterator<Item = &'a str>,
    operation: &str,
) -> Result<Output> {
    let output = Command::new("git")
        .arg("--no-pager")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to execute Git while attempting to {operation}"))?;
    check_git_output(output, operation)
}

fn run_git_owned(root: &Path, args: &[String], operation: &str) -> Result<Output> {
    let output = Command::new("git")
        .arg("--no-pager")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to execute Git while attempting to {operation}"))?;
    check_git_output(output, operation)
}

fn check_git_output(output: Output, operation: &str) -> Result<Output> {
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("failed to {operation}: {}", stderr.trim());
}

fn utf8_stdout<'a>(output: &'a Output, label: &str) -> Result<&'a str> {
    std::str::from_utf8(&output.stdout).with_context(|| format!("{label} is not valid UTF-8"))
}

fn parse_name_status(bytes: &[u8]) -> Result<Vec<(String, DiffFileStatus)>> {
    let mut fields = bytes.split(|byte| *byte == 0).filter(|field| !field.is_empty());
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let status = std::str::from_utf8(status).context("Git diff status is not valid UTF-8")?;
        let first = fields.next().ok_or_else(|| anyhow!("Git name-status record is truncated"))?;
        let first = std::str::from_utf8(first)
            .context("Git diff contains a non-UTF-8 path, which is unsupported")?
            .to_string();
        let record = match status.as_bytes().first().copied() {
            Some(b'A') => Some((first, DiffFileStatus::Added)),
            Some(b'M') => Some((first, DiffFileStatus::Modified)),
            Some(b'R') => {
                let destination =
                    fields.next().ok_or_else(|| anyhow!("Git rename record is truncated"))?;
                let destination = std::str::from_utf8(destination)
                    .context("Git diff contains a non-UTF-8 path, which is unsupported")?
                    .to_string();
                Some((destination, DiffFileStatus::Renamed { old_path: first }))
            }
            Some(_) | None => None,
        };
        if let Some(record) = record {
            files.push(record);
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files.dedup_by(|left, right| left.0 == right.0);
    Ok(files)
}

fn parse_added_ranges(bytes: &[u8]) -> Result<Vec<LineRange>> {
    let patch = std::str::from_utf8(bytes).context("Git patch output is not valid UTF-8")?;
    let mut ranges = Vec::new();
    for line in patch.lines().filter(|line| line.starts_with("@@ ")) {
        let Some(new_range) = line.split_whitespace().nth(2) else { continue };
        let Some(new_range) = new_range.strip_prefix('+') else { continue };
        let (start, count) = new_range.split_once(',').map_or((new_range, "1"), |parts| parts);
        let Ok(start) = start.parse::<u32>() else { continue };
        let Ok(count) = count.parse::<u32>() else { continue };
        if count == 0 {
            continue;
        }
        let end = start
            .checked_add(count - 1)
            .ok_or_else(|| anyhow!("Git hunk line range overflows u32"))?;
        ranges.push(LineRange { start, end });
    }
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<LineRange> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end.saturating_add(1)
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    Ok(merged)
}

fn load_dependency_additions(
    root: &Path,
    repo_root: &Path,
    root_prefix: &Path,
    resolved_base: &str,
    files: &BTreeMap<String, DiffFileScope>,
) -> Result<Vec<DependencyAddition>> {
    let mut additions = Vec::new();
    for file in files.values().filter(|file| file.path.ends_with("package.json")) {
        let current_source = fs::read_to_string(root.join(&file.path))
            .with_context(|| format!("cannot read changed manifest `{}`", file.path))?;
        let current: Value = serde_json::from_str(&current_source)
            .with_context(|| format!("changed manifest `{}` is malformed JSON", file.path))?;
        let base = if file.status == DiffFileStatus::Added {
            Value::Object(Map::new())
        } else {
            let base_path = match &file.status {
                DiffFileStatus::Renamed { old_path } => old_path,
                DiffFileStatus::Added | DiffFileStatus::Modified => &file.path,
            };
            let repo_path = root_prefix.join(base_path).to_string_lossy().replace('\\', "/");
            let spec = format!("{resolved_base}:{repo_path}");
            let output = run_git(
                repo_root,
                ["show", spec.as_str()],
                &format!("load base manifest `{base_path}`"),
            )?;
            let source = utf8_stdout(&output, "base package.json")?;
            serde_json::from_str(source)
                .with_context(|| format!("base manifest `{base_path}` is malformed JSON"))?
        };
        collect_dependency_additions(&file.path, &base, &current, &mut additions);
    }
    additions.sort();
    additions.dedup();
    Ok(additions)
}

fn collect_dependency_additions(
    manifest: &str,
    base: &Value,
    current: &Value,
    additions: &mut Vec<DependencyAddition>,
) {
    for section in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
        let base_entries = base.get(section).and_then(Value::as_object);
        let Some(current_entries) = current.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (name, requirement) in current_entries {
            if base_entries.is_some_and(|entries| entries.contains_key(name)) {
                continue;
            }
            additions.push(DependencyAddition {
                manifest: manifest.to_string(),
                section: section.to_string(),
                name: name.clone(),
                requirement: requirement
                    .as_str()
                    .map_or_else(|| requirement.to_string(), str::to_string),
            });
        }
    }
}

pub fn detect_diff_findings(facts: &ProjectFacts, scope: &DiffScope) -> Vec<SlopFinding> {
    let mut findings = Vec::new();
    if let Some(finding) = detect_scope_inflation(facts, scope) {
        findings.push(finding);
    }
    findings.extend(detect_introduced_reinvention(facts, scope));
    if let Some(finding) = detect_generated_surface_burst(facts, scope) {
        findings.push(finding);
    }
    findings.sort_by(|left, right| {
        left.span
            .cmp(&right.span)
            .then_with(|| left.kind.sort_rank().cmp(&right.kind.sort_rank()))
            .then_with(|| left.explanation.cmp(&right.explanation))
    });
    findings
}

fn detect_scope_inflation(facts: &ProjectFacts, scope: &DiffScope) -> Option<SlopFinding> {
    let behavior_lines = facts
        .files
        .values()
        .filter(|file| file.analysis_complete && !file.is_test && !file.is_generated)
        .flat_map(|file| {
            file.runtime_lines
                .iter()
                .filter(|line| scope.contains_line(&file.path, **line))
                .map(|line| (file.path.clone(), *line))
        })
        .collect::<BTreeSet<_>>();
    if behavior_lines.is_empty() || behavior_lines.len() > 20 {
        return None;
    }

    let new_files = scope
        .files
        .values()
        .filter(|file| file.status == DiffFileStatus::Added)
        .filter_map(|changed| {
            let file = facts.files.get(&changed.path)?;
            (file.analysis_complete
                && !file.is_test
                && !file.is_generated
                && !facts.entrypoints.contains(&file.path))
            .then_some(file)
        })
        .collect::<Vec<_>>();
    let mut helpers = facts
        .files
        .values()
        .filter(|file| file.analysis_complete && !file.is_test && !file.is_generated)
        .filter(|file| {
            scope.files.get(&file.path).is_some_and(|changed| {
                matches!(changed.status, DiffFileStatus::Modified | DiffFileStatus::Renamed { .. })
            })
        })
        .flat_map(|file| &file.declarations)
        .filter(|declaration| introduced_helper(facts, scope, declaration))
        .collect::<Vec<_>>();
    helpers.sort_by(|left, right| left.span.cmp(&right.span));
    if new_files.len() + helpers.len() < 3 && scope.dependency_additions.is_empty() {
        return None;
    }

    let (path, line) = behavior_lines.iter().next()?.clone();
    let primary = line_span(&path, line);
    let mut evidence = vec![SlopEvidence {
        code: "small-runtime-edit".to_string(),
        label: "runtime edit".to_string(),
        span: primary.clone(),
        detail: format!("The diff adds {} runtime line(s).", behavior_lines.len()),
    }];
    evidence.extend(new_files.into_iter().map(|file| SlopEvidence {
        code: "new-source-file".to_string(),
        label: "new source file".to_string(),
        span: line_span(&file.path, 1),
        detail: "This non-test, non-generated source file is added by the diff.".to_string(),
    }));
    evidence.extend(helpers.into_iter().map(|helper| SlopEvidence {
        code: "new-helper".to_string(),
        label: "new helper".to_string(),
        span: helper.span.clone(),
        detail: format!("The top-level helper `{}` is fully added.", helper.key.name),
    }));
    evidence.extend(scope.dependency_additions.iter().map(|dependency| SlopEvidence {
        code: "new-dependency".to_string(),
        label: "new dependency".to_string(),
        span: line_span(&dependency.manifest, 1),
        detail: format!(
            "`{}` adds `{}` with requirement `{}`.",
            dependency.section, dependency.name, dependency.requirement
        ),
    }));
    Some(SlopFinding {
        kind: SlopKind::ScopeInflation,
        confidence: SlopConfidence::Medium,
        span: primary,
        evidence,
        explanation: "The diff has a small runtime edit accompanied by broad new surface."
            .to_string(),
        action: "Review whether the added files, helpers, and dependencies are all needed for this runtime change."
            .to_string(),
    })
}

fn introduced_helper(
    facts: &ProjectFacts,
    scope: &DiffScope,
    declaration: &DeclarationFact,
) -> bool {
    declaration.kind == DeclarationKind::Function
        && declaration.role == Some(FunctionRole::Helper)
        && declaration.exported_as.is_empty()
        && !facts.entrypoints.contains(&declaration.key.path)
        && scope.fully_contains(&declaration.span)
        && facts
            .declaration_metadata
            .get(&declaration.key)
            .is_some_and(|metadata| metadata.top_level && !metadata.similarity_ignored)
}

fn detect_introduced_reinvention(facts: &ProjectFacts, scope: &DiffScope) -> Vec<SlopFinding> {
    let declarations = facts
        .files
        .values()
        .flat_map(|file| &file.declarations)
        .map(|declaration| (declaration.key.clone(), declaration))
        .collect::<BTreeMap<_, _>>();
    let mut candidates: BTreeMap<SymbolKey, Vec<(u16, &DeclarationFact)>> = BTreeMap::new();
    for match_ in &facts.reinvention_matches {
        if match_.similarity_millis < 870 {
            continue;
        }
        let (Some(candidate), Some(existing)) =
            (declarations.get(&match_.candidate), declarations.get(&match_.existing))
        else {
            continue;
        };
        let candidate_is_eligible = reinvention_candidate(facts, scope, candidate);
        let existing_is_eligible = reinvention_existing(facts, scope, existing);
        if !(candidate_is_eligible && existing_is_eligible)
            || candidate.key.path == existing.key.path
            || parameter_count(facts, candidate) != parameter_count(facts, existing)
            || parameter_count(facts, candidate).is_none()
        {
            continue;
        }
        candidates
            .entry(candidate.key.clone())
            .or_default()
            .push((match_.similarity_millis, existing));
    }

    let mut findings = Vec::new();
    for (candidate_key, mut existing) in candidates {
        existing
            .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.key.cmp(&right.1.key)));
        existing.dedup_by(|left, right| left.1.key == right.1.key);
        let candidate = declarations[&candidate_key];
        let mut evidence = vec![SlopEvidence {
            code: "introduced-helper".to_string(),
            label: "introduced helper".to_string(),
            span: candidate.span.clone(),
            detail: format!("The helper `{}` is fully added by the diff.", candidate.key.name),
        }];
        evidence.extend(existing.into_iter().map(|(similarity, declaration)| SlopEvidence {
            code: "referenced-existing-helper".to_string(),
            label: "referenced existing helper".to_string(),
            span: declaration.span.clone(),
            detail: format!(
                "`{}` has an offline structural similarity score of {}‰ and analyzed-project references.",
                declaration.key.name, similarity
            ),
        }));
        findings.push(SlopFinding {
            kind: SlopKind::IntroducedReinvention,
            confidence: SlopConfidence::Medium,
            span: candidate.span.clone(),
            evidence,
            explanation: "An added helper is structurally similar to a referenced helper outside the diff."
                .to_string(),
            action: "Compare the new helper with the referenced existing helper and reuse or consolidate only when their contracts match."
                .to_string(),
        });
    }
    findings
}

fn reinvention_candidate(
    facts: &ProjectFacts,
    scope: &DiffScope,
    declaration: &DeclarationFact,
) -> bool {
    file_is_eligible(facts, &declaration.key.path)
        && declaration.kind == DeclarationKind::Function
        && declaration.role == Some(FunctionRole::Helper)
        && !facts.entrypoints.contains(&declaration.key.path)
        && scope.fully_contains(&declaration.span)
        && facts
            .declaration_metadata
            .get(&declaration.key)
            .is_some_and(|metadata| metadata.top_level && !metadata.similarity_ignored)
}

fn reinvention_existing(
    facts: &ProjectFacts,
    scope: &DiffScope,
    declaration: &DeclarationFact,
) -> bool {
    file_is_eligible(facts, &declaration.key.path)
        && declaration.kind == DeclarationKind::Function
        && declaration.role == Some(FunctionRole::Helper)
        && !facts.entrypoints.contains(&declaration.key.path)
        && !scope.intersects(&declaration.span)
        && facts.symbol_uses.get(&declaration.key).is_some_and(|uses| !uses.is_empty())
        && facts.declaration_metadata.get(&declaration.key).is_some_and(|metadata| {
            metadata.top_level
                && metadata.reference_count_is_exact
                && !metadata.similarity_ignored
                && metadata.capture_count.is_none_or(|count| count == 0)
        })
}

fn parameter_count(facts: &ProjectFacts, declaration: &DeclarationFact) -> Option<usize> {
    facts
        .declaration_metadata
        .get(&declaration.key)
        .and_then(|metadata| metadata.parameter_count)
        .or(declaration.parameter_count)
}

fn detect_generated_surface_burst(facts: &ProjectFacts, scope: &DiffScope) -> Option<SlopFinding> {
    let mut surface = facts
        .files
        .values()
        .filter(|file| {
            file.analysis_complete
                && !file.is_test
                && !file.is_generated
                && !facts.entrypoints.contains(&file.path)
        })
        .flat_map(|file| &file.declarations)
        .filter(|declaration| {
            matches!(
                declaration.kind,
                DeclarationKind::Function
                    | DeclarationKind::Class
                    | DeclarationKind::Variable
                    | DeclarationKind::Interface
                    | DeclarationKind::TypeAlias
                    | DeclarationKind::Enum
            ) && !declaration.exported_as.is_empty()
                && scope.fully_contains(&declaration.span)
                && facts.symbol_uses.get(&declaration.key).is_none_or(Vec::is_empty)
                && facts
                    .declaration_metadata
                    .get(&declaration.key)
                    .is_some_and(|metadata| metadata.reference_count_is_exact)
        })
        .collect::<Vec<_>>();
    surface.sort_by(|left, right| left.span.cmp(&right.span));
    if surface.len() < 3 {
        surface.clear();
    }

    let mut test_groups = Vec::new();
    for group in duplicated_test_groups(facts) {
        if !group.literals_only_difference {
            continue;
        }
        let mut introduced = group
            .tests
            .iter()
            .filter_map(|id| facts.tests.get(id))
            .filter(|test| scope.fully_contains(&test.body_span))
            .collect::<Vec<_>>();
        introduced.sort_by(|left, right| left.id.cmp(&right.id));
        if introduced.len() >= 4 {
            test_groups.push(introduced);
        }
    }
    test_groups.sort_by(|left, right| left[0].id.cmp(&right[0].id));
    if surface.is_empty() && test_groups.is_empty() {
        return None;
    }

    let primary = surface.first().map_or_else(
        || test_groups[0][0].body_span.clone(),
        |declaration| declaration.span.clone(),
    );
    let mut evidence = surface
        .into_iter()
        .map(|declaration| SlopEvidence {
            code: "unreferenced-surface".to_string(),
            label: "unreferenced exported surface".to_string(),
            span: declaration.span.clone(),
            detail: format!(
                "No analyzed-project references were resolved for the fully added export `{}`.",
                declaration.key.name
            ),
        })
        .collect::<Vec<_>>();
    evidence.extend(test_groups.into_iter().flatten().map(|test| SlopEvidence {
        code: "near-identical-test".to_string(),
        label: "near-identical introduced test".to_string(),
        span: test.body_span.clone(),
        detail:
            "This fully added test differs from its normalized group only by literals.".to_string(),
    }));
    Some(SlopFinding {
        kind: SlopKind::GeneratedSurfaceBurst,
        confidence: SlopConfidence::Medium,
        span: primary,
        evidence,
        explanation: "The diff introduces a burst of exported surface with no resolved analyzed-project references, near-identical tests, or both."
            .to_string(),
        action: "Keep only the introduced surface and test cases that have distinct, demonstrated consumers or behavior."
            .to_string(),
    })
}

fn file_is_eligible(facts: &ProjectFacts, path: &str) -> bool {
    facts
        .files
        .get(path)
        .is_some_and(|file| file.analysis_complete && !file.is_test && !file.is_generated)
}

fn line_span(path: &str, line: u32) -> SourceSpan {
    SourceSpan {
        path: path.to_string(),
        start_byte: 0,
        end_byte: 0,
        start_line: line,
        start_column: 1,
        end_line: line,
        end_column: 1,
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git").args(args).current_dir(root).output().unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository() -> tempfile::TempDir {
        let root = tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        git(root.path(), &["config", "user.email", "scanr@example.invalid"]);
        git(root.path(), &["config", "user.name", "scanr test"]);
        fs::write(root.path().join("src.ts"), "export const first = 1;\n").unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"old":"1"},"devDependencies":{}}"#,
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "base"]);
        root
    }

    fn declaration(path: &str, name: &str, start: u32, kind: DeclarationKind) -> DeclarationFact {
        let span = line_span(path, start);
        DeclarationFact {
            key: SymbolKey {
                path: path.to_string(),
                declaration_start: start,
                name: name.to_string(),
            },
            span,
            body_span: None,
            scope: crate::slop::types::ScopeKey::Module(path.to_string()),
            kind,
            exported_as: Vec::new(),
            ambient: false,
            has_body: true,
            is_async: false,
            is_generator: false,
            role: (kind == DeclarationKind::Function).then_some(FunctionRole::Helper),
            body_shape: crate::slop::types::BodyShape::Other,
            parameter_count: None,
            branch_complexity: 1,
            control_nesting: 0,
            await_spans: Vec::new(),
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One fixture proves the three coordinated diff paths.
    fn all_three_diff_detectors_have_live_paths() {
        let mut scope_facts = ProjectFacts::default();
        scope_facts.files.insert(
            "behavior.ts".to_string(),
            crate::slop::types::FileFacts {
                path: "behavior.ts".to_string(),
                analysis_complete: true,
                runtime_lines: BTreeSet::from([2]),
                ..crate::slop::types::FileFacts::default()
            },
        );
        for path in ["a.ts", "b.ts", "c.ts"] {
            scope_facts.files.insert(
                path.to_string(),
                crate::slop::types::FileFacts {
                    path: path.to_string(),
                    analysis_complete: true,
                    ..crate::slop::types::FileFacts::default()
                },
            );
        }
        let mut scope_files = BTreeMap::from([(
            "behavior.ts".to_string(),
            DiffFileScope {
                path: "behavior.ts".to_string(),
                status: DiffFileStatus::Modified,
                added: vec![LineRange { start: 2, end: 2 }],
            },
        )]);
        for path in ["a.ts", "b.ts", "c.ts"] {
            scope_files.insert(
                path.to_string(),
                DiffFileScope {
                    path: path.to_string(),
                    status: DiffFileStatus::Added,
                    added: vec![LineRange { start: 1, end: 1 }],
                },
            );
        }
        let scope = DiffScope {
            requested_base: "HEAD".to_string(),
            resolved_base: "0".repeat(40),
            files: scope_files,
            dependency_additions: Vec::new(),
        };
        assert!(
            detect_diff_findings(&scope_facts, &scope)
                .iter()
                .any(|finding| finding.kind == SlopKind::ScopeInflation)
        );

        let mut reinvention_facts = ProjectFacts::default();
        let mut candidate = declaration("new.ts", "formatNew", 1, DeclarationKind::Function);
        candidate.span.end_line = 3;
        let existing = declaration("existing.ts", "formatExisting", 10, DeclarationKind::Function);
        for declaration in [&candidate, &existing] {
            reinvention_facts.declaration_metadata.insert(
                declaration.key.clone(),
                crate::slop::types::DeclarationMetadata {
                    top_level: true,
                    parameter_count: Some(1),
                    reference_count_is_exact: true,
                    ..crate::slop::types::DeclarationMetadata::default()
                },
            );
            reinvention_facts.files.insert(
                declaration.key.path.clone(),
                crate::slop::types::FileFacts {
                    path: declaration.key.path.clone(),
                    analysis_complete: true,
                    declarations: vec![declaration.clone()],
                    ..crate::slop::types::FileFacts::default()
                },
            );
        }
        reinvention_facts
            .symbol_uses
            .insert(existing.key.clone(), vec![line_span("consumer.ts", 1)]);
        reinvention_facts.reinvention_matches.push(crate::slop::types::ReinventionMatch {
            candidate: candidate.key,
            existing: existing.key,
            similarity_millis: 870,
        });
        let reinvention_scope = DiffScope {
            requested_base: "HEAD".to_string(),
            resolved_base: "0".repeat(40),
            files: BTreeMap::from([(
                "new.ts".to_string(),
                DiffFileScope {
                    path: "new.ts".to_string(),
                    status: DiffFileStatus::Modified,
                    added: vec![LineRange { start: 1, end: 3 }],
                },
            )]),
            dependency_additions: Vec::new(),
        };
        assert!(
            detect_diff_findings(&reinvention_facts, &reinvention_scope)
                .iter()
                .any(|finding| finding.kind == SlopKind::IntroducedReinvention)
        );

        let mut surface_facts = ProjectFacts::default();
        let mut declarations = ["First", "Second", "Third"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let mut declaration =
                    declaration("surface.ts", name, index as u32 + 1, DeclarationKind::Interface);
                declaration.exported_as.push(name.to_string());
                declaration
            })
            .collect::<Vec<_>>();
        for declaration in &declarations {
            surface_facts.declaration_metadata.insert(
                declaration.key.clone(),
                crate::slop::types::DeclarationMetadata {
                    top_level: true,
                    reference_count_is_exact: true,
                    ..crate::slop::types::DeclarationMetadata::default()
                },
            );
        }
        surface_facts.files.insert(
            "surface.ts".to_string(),
            crate::slop::types::FileFacts {
                path: "surface.ts".to_string(),
                analysis_complete: true,
                declarations: std::mem::take(&mut declarations),
                ..crate::slop::types::FileFacts::default()
            },
        );
        let surface_scope = DiffScope {
            requested_base: "HEAD".to_string(),
            resolved_base: "0".repeat(40),
            files: BTreeMap::from([(
                "surface.ts".to_string(),
                DiffFileScope {
                    path: "surface.ts".to_string(),
                    status: DiffFileStatus::Added,
                    added: vec![LineRange { start: 1, end: 3 }],
                },
            )]),
            dependency_additions: Vec::new(),
        };
        let surface = detect_diff_findings(&surface_facts, &surface_scope);
        let finding =
            surface.iter().find(|finding| finding.kind == SlopKind::GeneratedSurfaceBurst).unwrap();
        assert!(finding.explanation.contains("no resolved analyzed-project references"));
    }

    #[test]
    fn parses_hunks_and_merges_adjacent_ranges() {
        let patch =
            b"@@ -1 +1,2 @@\r\n@@ -8,0 +10 @@\n@@ -11 +12,0 @@\n@@ -20 +20,2 @@\n@@ -22 +22,2 @@\n";
        assert_eq!(
            parse_added_ranges(patch).unwrap(),
            vec![
                LineRange { start: 1, end: 2 },
                LineRange { start: 10, end: 10 },
                LineRange { start: 20, end: 23 },
            ]
        );
    }

    #[test]
    fn explicit_base_loads_staged_and_unstaged_lines_and_ignores_untracked() {
        let root = repository();
        fs::write(
            root.path().join("src.ts"),
            "export const first = 1;\nexport const staged = 2;\n",
        )
        .unwrap();
        git(root.path(), &["add", "src.ts"]);
        fs::write(
            root.path().join("src.ts"),
            "export const first = 1;\nexport const staged = 2;\nexport const unstaged = 3;\n",
        )
        .unwrap();
        fs::write(root.path().join("untracked.ts"), "export const ignored = 1;\n").unwrap();

        let scope = load_diff_scope(root.path(), "HEAD").unwrap();
        assert_eq!(scope.files["src.ts"].added, vec![LineRange { start: 2, end: 3 }]);
        assert!(!scope.files.contains_key("untracked.ts"));
        assert_eq!(scope, load_diff_scope(root.path(), "HEAD").unwrap());
    }

    #[test]
    fn tracks_added_files_renames_and_dependency_additions() {
        let root = repository();
        fs::write(
            root.path().join("src.ts"),
            "export const first = 1;\nexport const second = 2;\nexport const third = 3;\nexport const fourth = 4;\nexport const fifth = 5;\n",
        )
        .unwrap();
        git(root.path(), &["add", "src.ts"]);
        git(root.path(), &["commit", "-qm", "expand rename fixture"]);
        fs::write(root.path().join("added file.ts"), "export const added = 1;\n").unwrap();
        git(root.path(), &["add", "added file.ts"]);
        git(root.path(), &["mv", "src.ts", "renamed.ts"]);
        fs::write(
            root.path().join("renamed.ts"),
            "export const first = 1;\nexport const second = 2;\nexport const third = 3;\nexport const fourth = 4;\nexport const fifth = 5;\nexport const changed = 6;\n",
        )
        .unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"old":"2","new":"1"},"devDependencies":{"dev":"1"},"peerDependencies":{"peer":"1"},"optionalDependencies":{"optional":"1"}}"#,
        )
        .unwrap();

        let scope = load_diff_scope(root.path(), "HEAD").unwrap();
        assert_eq!(scope.files["added file.ts"].status, DiffFileStatus::Added);
        assert!(matches!(scope.files["renamed.ts"].status, DiffFileStatus::Renamed { .. }));
        assert_eq!(scope.files["renamed.ts"].added, vec![LineRange { start: 6, end: 6 }]);
        assert_eq!(scope.dependency_additions.len(), 4);
        assert!(scope.dependency_additions.iter().all(|addition| addition.name != "old"));
    }

    #[test]
    fn invalid_base_and_non_repository_are_errors() {
        let root = repository();
        let error = load_diff_scope(root.path(), "missing-ref").unwrap_err().to_string();
        assert!(error.contains("missing-ref"), "{error}");

        let outside = tempdir().unwrap();
        let error = load_diff_scope(outside.path(), "HEAD").unwrap_err().to_string();
        assert!(error.contains("requires a Git worktree"), "{error}");
    }
}
