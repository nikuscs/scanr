//! High-confidence exact slop detectors.

use std::collections::{BTreeMap, BTreeSet};

use crate::slop::types::{
    AsyncFact, CallResultUse, CastFact, CastKind, CatchEffectKind, CatchFact, CommentFact,
    ConstantCondition, Detector, ImportKey, ImportSpecifierKind, ProjectFacts, Resolution,
    ScopeKey, SlopConfidence, SlopEvidence, SlopFinding, SlopKind, SourceSpan,
    SuppressionDirectiveKind, SymbolKey, TestMode,
};

const SUPPRESSION_ACTION: &str = "Replace the suppression chain with a validated/narrowed type boundary, or document the one unavoidable suppression at that boundary.";

#[allow(dead_code)] // The command registry is synchronized by its owning integration lane.
pub fn detectors() -> Vec<Box<dyn Detector>> {
    vec![
        Box::new(SuppressionChainDetector),
        Box::new(SwallowedFailureDetector),
        Box::new(UnresolvedApiDetector),
        Box::new(AsyncMisuseDetector),
        Box::new(DeadSurfaceDetector),
        Box::new(NonExecutingTestDetector),
    ]
}

struct SuppressionChainDetector;
struct SwallowedFailureDetector;
struct UnresolvedApiDetector;
struct AsyncMisuseDetector;
struct DeadSurfaceDetector;
struct NonExecutingTestDetector;

impl Detector for SuppressionChainDetector {
    fn kind(&self) -> SlopKind {
        SlopKind::SuppressionChain
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::High
    }

    fn detect(&self, facts: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in facts.files.values().filter(|file| file.analysis_complete && !file.is_generated)
        {
            let mut consumed = BTreeSet::new();
            let mut expression_groups: BTreeMap<SourceSpan, Vec<&CastFact>> = BTreeMap::new();
            for cast in &file.casts {
                if is_suppression_cast(cast) {
                    expression_groups.entry(cast.expression_root.clone()).or_default().push(cast);
                }
            }

            for (root, mut casts) in expression_groups {
                casts.sort_by(|left, right| {
                    left.span.cmp(&right.span).then(left.kind.cmp(&right.kind))
                });
                casts.dedup_by(|left, right| left.span == right.span && left.kind == right.kind);
                let emits = casts.iter().any(|cast| {
                    matches!(cast.kind, CastKind::AsAny | CastKind::TypeAssertionAny)
                        || cast.nested_assertion_count >= 2
                }) || casts.len() >= 2;
                if !emits {
                    continue;
                }
                let mut evidence = vec![evidence(
                    suppression_subtype(&casts),
                    "suppression expression",
                    root.clone(),
                    suppression_expression_detail(&casts),
                )];
                for cast in &casts {
                    consumed.insert(cast_identity(cast));
                    evidence.push(cast_evidence(cast));
                }
                findings.push(finding(
                    SlopKind::SuppressionChain,
                    root,
                    evidence,
                    "This expression combines syntax that bypasses or overrides static checks.",
                    SUPPRESSION_ACTION,
                ));
            }

            let mut scope_atoms: BTreeMap<ScopeKey, Vec<SlopEvidence>> = BTreeMap::new();
            for cast in &file.casts {
                if is_suppression_cast(cast) && !consumed.contains(&cast_identity(cast)) {
                    scope_atoms.entry(cast.scope.clone()).or_default().push(cast_evidence(cast));
                }
            }
            let mut seen_directives = BTreeSet::new();
            for comment in &file.comments {
                let Some(directive) = comment.directive else { continue };
                let identity = (
                    comment.scope.clone(),
                    directive,
                    comment.target.as_ref().map(|span| (span.start_byte, span.end_byte)),
                );
                if seen_directives.insert(identity) {
                    scope_atoms
                        .entry(comment.scope.clone())
                        .or_default()
                        .push(comment_evidence(comment, directive));
                }
            }

            for (_, mut atoms) in scope_atoms {
                normalize_evidence(&mut atoms);
                let broad_directive = atoms.iter().any(|item| item.code == "eslint-disable");
                if atoms.len() < 2 && !broad_directive {
                    continue;
                }
                let primary = atoms[0].span.clone();
                findings.push(finding(
                    SlopKind::SuppressionChain,
                    primary,
                    atoms,
                    "This scope contains multiple static-check suppressions, or a directive that disables lint checking beyond one expression.",
                    SUPPRESSION_ACTION,
                ));
            }
        }
        normalize_findings(&mut findings);
        findings
    }
}

impl Detector for SwallowedFailureDetector {
    fn kind(&self) -> SlopKind {
        SlopKind::SwallowedFailure
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::High
    }

    fn detect(&self, facts: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in facts.files.values().filter(|file| file.analysis_complete && !file.is_generated)
        {
            for catch in &file.catches {
                if let Some((code, explanation)) = swallowed_shape(catch, "catch") {
                    findings.push(swallowed_finding(catch, code, explanation));
                }
            }
            for promise in &file.promise_catches {
                if let Resolution::Resolved(callback) = &promise.callback
                    && let Some((code, explanation)) = swallowed_shape(callback, "promise-catch")
                {
                    findings.push(swallowed_finding(callback, code, explanation));
                }
            }
        }
        normalize_findings(&mut findings);
        findings
    }
}

type MissingModuleUses = BTreeMap<(String, String), Vec<(SourceSpan, Vec<String>)>>;

impl Detector for UnresolvedApiDetector {
    fn kind(&self) -> SlopKind {
        SlopKind::UnresolvedApi
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::High
    }

    #[allow(clippy::too_many_lines)] // Collection and rendering stay together to preserve grouping keys.
    fn detect(&self, facts: &ProjectFacts) -> Vec<SlopFinding> {
        let mut missing_modules = MissingModuleUses::new();
        let mut missing_exports: BTreeMap<(String, String), Vec<SourceSpan>> = BTreeMap::new();
        let mut missing_members: BTreeMap<(String, String), Vec<SourceSpan>> = BTreeMap::new();

        for (key, resolution) in &facts.imports {
            let Some(import) = import_fact(facts, key) else { continue };
            if !facts.files.get(&key.importer).is_some_and(|file| file.analysis_complete) {
                continue;
            }
            match &resolution.module {
                Resolution::Missing { attempted } => {
                    let generated_source = key.source.contains(".gen")
                        || key.source.contains(".generated")
                        || key.source.split('/').any(|segment| segment == "generated");
                    let has_local_candidate = key.source.starts_with('.')
                        || attempted.iter().any(|candidate| {
                            !candidate.starts_with("node_modules/")
                                && !candidate.contains("/node_modules/")
                        });
                    if has_local_candidate && !generated_source {
                        missing_modules
                            .entry((key.importer.clone(), key.source.clone()))
                            .or_default()
                            .push((import.span.clone(), sorted_unique(attempted.clone())));
                    }
                }
                Resolution::Resolved(module) => {
                    if import.kind != ImportSpecifierKind::SideEffect
                        && !import.type_only
                        && matches!(resolution.export, Resolution::Missing { .. })
                        && export_surface_complete(facts, module)
                    {
                        let requested =
                            key.imported.clone().unwrap_or_else(|| "default".to_string());
                        missing_exports
                            .entry((module.clone(), requested))
                            .or_default()
                            .push(import.span.clone());
                    }
                    if import.kind == ImportSpecifierKind::Namespace
                        && !import.type_only
                        && let Some(local) = &key.local
                        && export_surface_complete(facts, module)
                        && let Some(file) = facts.files.get(&key.importer)
                    {
                        for member in &file.member_uses {
                            let Some(name) = &member.static_member else { continue };
                            if member.base_name.as_ref() == Some(local)
                                && !facts.exports[module].names.contains_key(name)
                                && name != "default"
                            {
                                missing_members
                                    .entry((module.clone(), name.clone()))
                                    .or_default()
                                    .push(member.span.clone());
                            }
                        }
                    }
                }
                Resolution::Ambiguous { .. } | Resolution::Unknown { .. } => {}
            }
        }

        let mut findings = Vec::new();
        for ((_, source), mut uses) in missing_modules {
            uses.sort_by(|left, right| left.0.cmp(&right.0));
            let primary = uses[0].0.clone();
            let mut items = vec![evidence(
                "missing-module",
                "unresolved import",
                primary.clone(),
                format!("The import source `{source}` did not resolve to a module."),
            )];
            for (span, attempted) in uses {
                items.push(evidence(
                    "import-use",
                    "import site",
                    span.clone(),
                    format!("This site imports `{source}`."),
                ));
                for candidate in attempted {
                    items.push(evidence(
                        "attempted-module",
                        "attempted module path",
                        span.clone(),
                        format!("Resolution checked `{candidate}`."),
                    ));
                }
            }
            findings.push(finding(
                SlopKind::UnresolvedApi,
                primary,
                items,
                "The import could not be resolved from any deterministic local or package candidate.",
                "Correct the import path or add the module that the code requires.",
            ));
        }
        for ((module, requested), mut spans) in missing_exports {
            spans.sort();
            spans.dedup();
            let primary = spans[0].clone();
            let evidence = spans
                .into_iter()
                .map(|span| {
                    evidence(
                        "missing-export",
                        "missing export",
                        span,
                        format!("`{module}` has a complete export surface without `{requested}`."),
                    )
                })
                .collect();
            findings.push(finding(
                SlopKind::UnresolvedApi,
                primary,
                evidence,
                "The resolved module does not export the statically requested name.",
                "Use an existing export or add the missing export to the resolved module.",
            ));
        }
        for ((module, member), mut spans) in missing_members {
            spans.sort();
            spans.dedup();
            let primary = spans[0].clone();
            let evidence = spans
                .into_iter()
                .map(|span| {
                    evidence(
                        "missing-namespace-member",
                        "missing namespace member",
                        span,
                        format!("`{module}` has a complete export surface without `{member}`."),
                    )
                })
                .collect();
            findings.push(finding(
                SlopKind::UnresolvedApi,
                primary,
                evidence,
                "A statically named namespace member is absent from the resolved module's complete export surface.",
                "Use an exported namespace member or add the missing export.",
            ));
        }
        normalize_findings(&mut findings);
        findings
    }
}

impl Detector for AsyncMisuseDetector {
    fn kind(&self) -> SlopKind {
        SlopKind::AsyncMisuse
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::High
    }

    fn detect(&self, facts: &ProjectFacts) -> Vec<SlopFinding> {
        let declarations = facts
            .files
            .values()
            .flat_map(|file| file.declarations.iter())
            .map(|declaration| (declaration.key.clone(), declaration))
            .collect::<BTreeMap<_, _>>();
        let mut findings = Vec::new();
        for file in facts.files.values().filter(|file| file.analysis_complete && !file.is_generated)
        {
            for call in &file.async_calls {
                if call.result_use != CallResultUse::FloatingExpressionStatement {
                    continue;
                }
                let has_rejection_handler = file.promise_catches.iter().any(|handler| {
                    handler.call_span.start_byte <= call.span.start_byte
                        && handler.call_span.end_byte >= call.span.end_byte
                        || call.span.start_byte <= handler.call_span.start_byte
                            && call.span.end_byte >= handler.call_span.end_byte
                });
                if has_rejection_handler {
                    continue;
                }
                let Some(target) = resolved_call_target(call, facts) else { continue };
                let Some(declaration) = declarations.get(target) else { continue };
                if !declaration.is_async || !declaration.has_body || declaration.ambient {
                    continue;
                }
                findings.push(finding(
                    SlopKind::AsyncMisuse,
                    call.span.clone(),
                    vec![
                        evidence(
                            "floating-known-async-call",
                            "floating async call",
                            call.span.clone(),
                            format!("The result of `{}` is not handled.", display_callee(call)),
                        ),
                        evidence(
                            "async-declaration",
                            "resolved async declaration",
                            declaration.span.clone(),
                            format!("`{}` is declared async with an implementation body.", target.name),
                        ),
                    ],
                    "This expression discards the result of an exactly resolved async function call.",
                    "Await, return, explicitly `void`, or attach an intentional rejection handler to this promise.",
                ));
            }
        }
        normalize_findings(&mut findings);
        findings
    }
}

impl Detector for DeadSurfaceDetector {
    fn kind(&self) -> SlopKind {
        SlopKind::DeadSurface
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::High
    }

    fn detect(&self, facts: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in facts.files.values().filter(|file| file.analysis_complete && !file.is_generated)
        {
            for branch in &file.branches {
                let (Some(condition), Some(unreachable), Some(comment)) = (
                    branch.condition,
                    branch.unreachable_span.as_ref(),
                    branch.adjacent_placeholder_comment.as_ref(),
                ) else {
                    continue;
                };
                let detail = match condition {
                    ConstantCondition::AlwaysTrue => {
                        "The literal `true` condition makes the alternate branch unreachable."
                    }
                    ConstantCondition::AlwaysFalse => {
                        "The literal `false` condition makes the consequent branch unreachable."
                    }
                };
                findings.push(finding(
                    SlopKind::DeadSurface,
                    branch.condition_span.clone(),
                    vec![
                        evidence(
                            "literal-placeholder-branch",
                            "constant branch condition",
                            branch.condition_span.clone(),
                            detail,
                        ),
                        evidence(
                            "placeholder-comment",
                            "adjacent placeholder marker",
                            comment.clone(),
                            "An adjacent TODO, FIXME, or placeholder comment marks this branch as unfinished.",
                        ),
                        evidence(
                            "unreachable-branch",
                            "unreachable branch",
                            unreachable.clone(),
                            "This branch cannot execute under the literal condition.",
                        ),
                    ],
                    "A literal condition and adjacent placeholder marker prove that one branch cannot execute.",
                    "Remove the unreachable placeholder branch or replace the literal with the intended condition.",
                ));
            }
        }
        normalize_findings(&mut findings);
        findings
    }
}

impl Detector for NonExecutingTestDetector {
    fn kind(&self) -> SlopKind {
        SlopKind::NonExecutingTest
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::High
    }

    fn detect(&self, facts: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in facts.files.values().filter(|file| file.analysis_complete && !file.is_generated)
        {
            for test in &file.tests {
                if !matches!(test.mode, TestMode::Skip | TestMode::Todo | TestMode::DisabledAlias) {
                    continue;
                }
                let mode = match test.mode {
                    TestMode::Skip => "skipped",
                    TestMode::Todo => "todo",
                    TestMode::DisabledAlias => "disabled alias",
                    _ => unreachable!(),
                };
                let name = test.name.as_deref().unwrap_or("unnamed test");
                findings.push(finding(
                    SlopKind::NonExecutingTest,
                    test.call_span.clone(),
                    vec![evidence(
                        "skipped-test",
                        "non-executing test registration",
                        test.call_span.clone(),
                        format!("The recognized runner registers `{name}` as {mode}."),
                    )],
                    "The recognized test mode prevents this test body from executing.",
                    "Enable the test and make it pass, or remove it if it is no longer required.",
                ));
            }
        }
        normalize_findings(&mut findings);
        findings
    }
}

const fn is_suppression_cast(cast: &CastFact) -> bool {
    matches!(cast.kind, CastKind::AsAny | CastKind::TypeAssertionAny | CastKind::NonNull)
        || cast.nested_assertion_count >= 2
}

const fn cast_identity(cast: &CastFact) -> (u32, u32, CastKind) {
    (cast.span.start_byte, cast.span.end_byte, cast.kind)
}

fn suppression_subtype(casts: &[&CastFact]) -> &'static str {
    if casts.iter().any(|cast| matches!(cast.kind, CastKind::AsAny | CastKind::TypeAssertionAny)) {
        "as-any"
    } else if casts.iter().any(|cast| cast.nested_assertion_count >= 2) {
        "double-assertion"
    } else {
        "non-null"
    }
}

fn suppression_expression_detail(casts: &[&CastFact]) -> String {
    if casts.iter().any(|cast| matches!(cast.kind, CastKind::AsAny | CastKind::TypeAssertionAny)) {
        "This expression includes an explicit assertion to `any`.".to_string()
    } else if casts.iter().any(|cast| cast.nested_assertion_count >= 2) {
        "This expression contains a nested assertion chain.".to_string()
    } else {
        "This expression combines multiple non-null or assertion operations.".to_string()
    }
}

fn cast_evidence(cast: &CastFact) -> SlopEvidence {
    let (code, label, detail) = match cast.kind {
        CastKind::AsAny | CastKind::TypeAssertionAny => (
            "as-any",
            "assertion to any",
            "This assertion explicitly widens the expression to `any`.".to_string(),
        ),
        CastKind::NonNull => (
            "non-null",
            "non-null assertion",
            "This assertion suppresses a possible null or undefined value.".to_string(),
        ),
        CastKind::OtherAs | CastKind::OtherTypeAssertion => (
            "double-assertion",
            "nested type assertion",
            format!(
                "This assertion is part of a chain containing {} assertions.",
                cast.nested_assertion_count
            ),
        ),
    };
    evidence(code, label, cast.span.clone(), detail)
}

fn comment_evidence(comment: &CommentFact, directive: SuppressionDirectiveKind) -> SlopEvidence {
    let (code, label, detail) = match directive {
        SuppressionDirectiveKind::TsIgnore => (
            "ts-ignore",
            "TypeScript suppression",
            "This directive suppresses the next TypeScript diagnostic.".to_string(),
        ),
        SuppressionDirectiveKind::EslintDisable
        | SuppressionDirectiveKind::EslintDisableNextLine => {
            let scope = if directive == SuppressionDirectiveKind::EslintDisable {
                "a broader lint scope"
            } else {
                "the next line"
            };
            let rules = if comment.lint_rules.is_empty() {
                "all configured rules".to_string()
            } else {
                comment.lint_rules.join(", ")
            };
            (
                "eslint-disable",
                "lint suppression",
                format!("This directive disables {rules} for {scope}."),
            )
        }
    };
    evidence(code, label, comment.span.clone(), detail)
}

fn swallowed_shape(catch: &CatchFact, suffix: &str) -> Option<(&'static str, &'static str)> {
    if catch.top_level_statement_count == 0 && catch.effects.is_empty() {
        return Some(if suffix == "promise-catch" {
            (
                "empty-promise-catch",
                "This exactly resolved promise rejection callback has an empty body, so it neither propagates nor handles the failure.",
            )
        } else {
            (
                "empty-catch",
                "This catch block has an empty body, so it neither propagates nor handles the failure.",
            )
        });
    }
    let log_only = !catch.effects.is_empty()
        && catch.can_fall_through
        && !catch.has_nested_function
        && catch
            .effects
            .iter()
            .all(|(effect, _)| matches!(effect, CatchEffectKind::Log | CatchEffectKind::Telemetry));
    log_only.then_some(if suffix == "promise-catch" {
        (
            "log-only-promise-catch",
            "This exactly resolved promise rejection callback only logs or records telemetry and then falls through.",
        )
    } else {
        (
            "log-only-catch",
            "This catch block only logs or records telemetry and then falls through.",
        )
    })
}

fn swallowed_finding(
    catch: &CatchFact,
    code: &'static str,
    explanation: &'static str,
) -> SlopFinding {
    let mut items =
        vec![evidence(code, "swallowed failure handler", catch.span.clone(), explanation)];
    for (effect, span) in &catch.effects {
        let (effect_code, label, detail) = match effect {
            CatchEffectKind::Log => {
                ("log-effect", "log call", "This call logs the failure but does not propagate it.")
            }
            CatchEffectKind::Telemetry => (
                "telemetry-effect",
                "telemetry call",
                "This call records the failure but does not propagate it.",
            ),
            _ => continue,
        };
        items.push(evidence(effect_code, label, span.clone(), detail));
    }
    finding(
        SlopKind::SwallowedFailure,
        catch.span.clone(),
        items,
        explanation,
        "Propagate the failure, return an explicit failure result, or perform a clearly intentional recovery.",
    )
}

fn import_fact<'a>(
    facts: &'a ProjectFacts,
    key: &ImportKey,
) -> Option<&'a crate::slop::types::ImportFact> {
    facts.files.get(&key.importer)?.imports.iter().find(|import| {
        import.span.start_byte == key.start_byte
            && import.source == key.source
            && import.imported == key.imported
            && import.local == key.local
    })
}

fn export_surface_complete(facts: &ProjectFacts, module: &str) -> bool {
    facts.exports.get(module).is_some_and(|surface| {
        surface.complete
            && !surface.common_js_unknown
            && surface.unknown_star_reexports.is_empty()
            && !facts.coverage.parse_incomplete_files.contains(module)
    })
}

fn resolved_call_target<'a>(call: &'a AsyncFact, facts: &'a ProjectFacts) -> Option<&'a SymbolKey> {
    match &call.callee_symbol {
        Resolution::Resolved(target) => Some(target),
        _ => match facts.call_targets.get(&call.key) {
            Some(Resolution::Resolved(target)) => Some(target),
            _ => None,
        },
    }
}

fn display_callee(call: &AsyncFact) -> String {
    if call.callee_path.is_empty() {
        "resolved async function".to_string()
    } else {
        call.callee_path.join(".")
    }
}

fn evidence(
    code: impl Into<String>,
    label: impl Into<String>,
    span: SourceSpan,
    detail: impl Into<String>,
) -> SlopEvidence {
    SlopEvidence { code: code.into(), label: label.into(), span, detail: detail.into() }
}

fn finding(
    kind: SlopKind,
    span: SourceSpan,
    mut evidence: Vec<SlopEvidence>,
    explanation: impl Into<String>,
    action: impl Into<String>,
) -> SlopFinding {
    let primary = evidence.first().cloned();
    normalize_evidence(&mut evidence);
    if let Some(index) = evidence.iter().position(|item| Some(item) == primary.as_ref()) {
        evidence.swap(0, index);
    }
    SlopFinding {
        kind,
        confidence: SlopConfidence::High,
        span,
        evidence,
        explanation: explanation.into(),
        action: action.into(),
    }
}

fn normalize_evidence(evidence: &mut Vec<SlopEvidence>) {
    evidence.sort_by(|left, right| {
        left.span
            .cmp(&right.span)
            .then(left.code.cmp(&right.code))
            .then(left.detail.cmp(&right.detail))
    });
    evidence.dedup_by(|left, right| {
        left.code == right.code && left.span == right.span && left.detail == right.detail
    });
}

fn normalize_findings(findings: &mut Vec<SlopFinding>) {
    for finding in findings.iter_mut() {
        let primary = finding.evidence.first().cloned();
        normalize_evidence(&mut finding.evidence);
        if let Some(index) = finding.evidence.iter().position(|item| Some(item) == primary.as_ref())
        {
            finding.evidence.swap(0, index);
        }
    }
    findings.sort_by(|left, right| {
        left.span
            .cmp(&right.span)
            .then(left.kind.cmp(&right.kind))
            .then_with(|| left.evidence[0].code.cmp(&right.evidence[0].code))
    });
    findings.dedup_by(|left, right| {
        left.kind == right.kind
            && left.span == right.span
            && left.evidence.first().map(|item| &item.code)
                == right.evidence.first().map(|item| &item.code)
    });
}

fn sorted_unique<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slop::types::{
        AnalysisCoverage, BranchFact, CallSiteKey, CatchFact, DeclarationFact, DeclarationKind,
        ExportSurface, FileFacts, ImportFact, ImportResolution, MemberUseFact, PromiseCatchFact,
        ReturnShape, TestFact,
    };
    use crate::slop::types::{BodyShape, CallResultUse};

    fn span(path: &str, start: u32, end: u32) -> SourceSpan {
        SourceSpan {
            path: path.to_string(),
            start_byte: start,
            end_byte: end,
            start_line: start + 1,
            start_column: 1,
            end_line: end + 1,
            end_column: 1,
        }
    }

    fn scope(path: &str) -> ScopeKey {
        ScopeKey::Module(path.to_string())
    }

    fn file(path: &str) -> FileFacts {
        FileFacts {
            path: path.to_string(),
            analysis_complete: true,
            export_surface: ExportSurface { complete: true, ..ExportSurface::default() },
            ..FileFacts::default()
        }
    }

    fn project(files: Vec<FileFacts>) -> ProjectFacts {
        let mut project = ProjectFacts::default();
        for file in files {
            project.exports.insert(file.path.clone(), file.export_surface.clone());
            project.files.insert(file.path.clone(), file);
        }
        project
    }

    fn cast(path: &str, start: u32, kind: CastKind) -> CastFact {
        CastFact {
            span: span(path, start, start + 2),
            operand_span: span(path, start, start + 1),
            expression_root: span(path, start, start + 2),
            scope: scope(path),
            kind,
            nesting_depth: 1,
            nested_assertion_count: 1,
            target_type: String::new(),
        }
    }

    #[test]
    fn registry_contains_all_six_exact_detectors_in_stable_order() {
        let kinds = detectors().into_iter().map(|detector| detector.kind()).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                SlopKind::SuppressionChain,
                SlopKind::SwallowedFailure,
                SlopKind::UnresolvedApi,
                SlopKind::AsyncMisuse,
                SlopKind::DeadSurface,
                SlopKind::NonExecutingTest,
            ]
        );
    }

    #[test]
    fn suppression_reports_as_any_and_suppresses_isolated_non_null() {
        let mut facts = file("a.ts");
        facts.casts.push(cast("a.ts", 2, CastKind::AsAny));
        facts.casts.push(cast("a.ts", 20, CastKind::NonNull));
        let findings = SuppressionChainDetector.detect(&project(vec![facts]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence[0].code, "as-any");
        assert_eq!(findings[0].span.start_byte, 2);
    }

    #[test]
    fn suppression_groups_scope_atoms_and_deduplicates_attached_directives() {
        let mut facts = file("a.ts");
        facts.casts.push(cast("a.ts", 20, CastKind::NonNull));
        let directive = CommentFact {
            span: span("a.ts", 2, 4),
            scope: scope("a.ts"),
            directive: Some(SuppressionDirectiveKind::TsIgnore),
            lint_rules: Vec::new(),
            target: Some(span("a.ts", 10, 11)),
            placeholder: false,
            narrates_trivial: false,
        };
        facts.comments.extend([directive.clone(), directive]);
        let findings = SuppressionChainDetector.detect(&project(vec![facts]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence.len(), 2);
        assert_eq!(findings[0].evidence[0].code, "ts-ignore");
    }

    fn catch(path: &str, start: u32, effects: Vec<(CatchEffectKind, SourceSpan)>) -> CatchFact {
        CatchFact {
            span: span(path, start, start + 10),
            body_span: span(path, start + 2, start + 9),
            scope: scope(path),
            parameter_name: Some("error".to_string()),
            top_level_statement_count: effects.len(),
            effects,
            return_shape: ReturnShape::None,
            can_fall_through: true,
            has_nested_function: false,
        }
    }

    #[test]
    fn swallowed_failure_reports_empty_and_log_only_but_not_unknown_recovery() {
        let mut facts = file("a.ts");
        facts.catches.push(catch("a.ts", 1, Vec::new()));
        facts.catches.push(catch("a.ts", 20, vec![(CatchEffectKind::Log, span("a.ts", 23, 25))]));
        facts.catches.push(catch(
            "a.ts",
            40,
            vec![(CatchEffectKind::OtherCall, span("a.ts", 43, 45))],
        ));
        let findings = SwallowedFailureDetector.detect(&project(vec![facts]));
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].evidence[0].code, "empty-catch");
        assert_eq!(findings[1].evidence[0].code, "log-only-catch");
    }

    #[test]
    fn swallowed_failure_requires_resolved_promise_callback() {
        let mut facts = file("a.ts");
        facts.promise_catches.push(PromiseCatchFact {
            call_span: span("a.ts", 1, 10),
            callback_span: None,
            scope: scope("a.ts"),
            callback: Resolution::Unknown { reason: "not summarized".to_string() },
        });
        assert!(SwallowedFailureDetector.detect(&project(vec![facts])).is_empty());
    }

    fn import(
        path: &str,
        start: u32,
        source: &str,
        kind: ImportSpecifierKind,
        imported: Option<&str>,
        local: Option<&str>,
    ) -> (ImportKey, ImportFact) {
        let key = ImportKey {
            importer: path.to_string(),
            source: source.to_string(),
            imported: imported.map(str::to_string),
            local: local.map(str::to_string),
            start_byte: start,
        };
        let fact = ImportFact {
            span: span(path, start, start + 2),
            source: source.to_string(),
            kind,
            imported: imported.map(str::to_string),
            local: local.map(str::to_string),
            type_only: false,
        };
        (key, fact)
    }

    #[test]
    fn unresolved_api_reports_only_proven_missing_module_and_export() {
        let (missing_key, missing_fact) =
            import("a.ts", 1, "./missing", ImportSpecifierKind::Named, Some("save"), Some("save"));
        let (export_key, export_fact) =
            import("a.ts", 10, "./b", ImportSpecifierKind::Named, Some("save"), Some("save2"));
        let (external_key, external_fact) = import(
            "a.ts",
            20,
            "react",
            ImportSpecifierKind::Named,
            Some("useState"),
            Some("useState"),
        );
        let (type_key, mut type_fact) = import(
            "a.ts",
            30,
            "./b",
            ImportSpecifierKind::Named,
            Some("MissingType"),
            Some("MissingType"),
        );
        type_fact.type_only = true;
        let (generated_key, generated_fact) = import(
            "a.ts",
            40,
            "./routeTree.gen",
            ImportSpecifierKind::Default,
            None,
            Some("routeTree"),
        );
        let mut importer = file("a.ts");
        importer.imports.extend([
            missing_fact,
            export_fact,
            external_fact,
            type_fact,
            generated_fact,
        ]);
        let target = file("b.ts");
        let mut project = project(vec![importer, target]);
        project.imports.insert(
            missing_key,
            ImportResolution {
                module: Resolution::Missing {
                    attempted: vec!["missing.ts".to_string(), "missing.ts".to_string()],
                },
                export: Resolution::Missing { attempted: Vec::new() },
                resolved_symbol: None,
            },
        );
        project.imports.insert(
            export_key,
            ImportResolution {
                module: Resolution::Resolved("b.ts".to_string()),
                export: Resolution::Missing { attempted: vec!["b.ts#save".to_string()] },
                resolved_symbol: None,
            },
        );
        project.imports.insert(
            external_key,
            ImportResolution {
                module: Resolution::Missing {
                    attempted: vec!["node_modules/react/index.d.ts".to_string()],
                },
                export: Resolution::Missing { attempted: Vec::new() },
                resolved_symbol: None,
            },
        );
        project.imports.insert(
            type_key,
            ImportResolution {
                module: Resolution::Resolved("b.ts".to_string()),
                export: Resolution::Missing { attempted: vec!["b.ts#MissingType".to_string()] },
                resolved_symbol: None,
            },
        );
        project.imports.insert(
            generated_key,
            ImportResolution {
                module: Resolution::Missing { attempted: vec!["routeTree.gen.ts".to_string()] },
                export: Resolution::Missing { attempted: Vec::new() },
                resolved_symbol: None,
            },
        );
        let findings = UnresolvedApiDetector.detect(&project);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].evidence[0].code, "missing-module");
        assert_eq!(findings[1].evidence[0].code, "missing-export");
    }

    #[test]
    fn unresolved_api_groups_missing_namespace_member_uses() {
        let (key, fact) =
            import("a.ts", 1, "./b", ImportSpecifierKind::Namespace, Some("*"), Some("api"));
        let mut importer = file("a.ts");
        importer.imports.push(fact);
        importer.member_uses.extend([5, 20].map(|start| MemberUseFact {
            span: span("a.ts", start, start + 2),
            scope: scope("a.ts"),
            base_name: Some("api".to_string()),
            static_member: Some("missing".to_string()),
        }));
        let target = file("b.ts");
        let mut project = project(vec![importer, target]);
        project.imports.insert(
            key,
            ImportResolution {
                module: Resolution::Resolved("b.ts".to_string()),
                export: Resolution::Unknown { reason: "namespace".to_string() },
                resolved_symbol: None,
            },
        );
        let findings = UnresolvedApiDetector.detect(&project);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence.len(), 2);
        assert_eq!(findings[0].evidence[0].code, "missing-namespace-member");
    }

    fn declaration(path: &str, start: u32, name: &str) -> DeclarationFact {
        DeclarationFact {
            key: SymbolKey {
                path: path.to_string(),
                declaration_start: start,
                name: name.to_string(),
            },
            span: span(path, start, start + 5),
            body_span: Some(span(path, start + 2, start + 5)),
            scope: scope(path),
            kind: DeclarationKind::Function,
            exported_as: Vec::new(),
            ambient: false,
            has_body: true,
            is_async: true,
            is_generator: false,
            role: None,
            body_shape: BodyShape::Other,
            parameter_count: None,
            branch_complexity: 1,
            control_nesting: 0,
            await_spans: Vec::new(),
        }
    }

    #[test]
    fn async_misuse_reports_only_floating_exact_async_calls() {
        let declaration = declaration("a.ts", 1, "save");
        let target = declaration.key.clone();
        let mut facts = file("a.ts");
        facts.declarations.push(declaration);
        for (start, result_use) in [
            (20, CallResultUse::FloatingExpressionStatement),
            (30, CallResultUse::Awaited),
            (40, CallResultUse::FloatingExpressionStatement),
        ] {
            facts.async_calls.push(AsyncFact {
                key: CallSiteKey { path: "a.ts".to_string(), start_byte: start },
                span: span("a.ts", start, start + 2),
                scope: scope("a.ts"),
                callee_path: vec!["save".to_string()],
                callee_symbol: Resolution::Resolved(target.clone()),
                result_use,
                nearest_loop: None,
                await_span: None,
            });
        }
        facts.promise_catches.push(PromiseCatchFact {
            call_span: span("a.ts", 39, 50),
            callback_span: None,
            scope: scope("a.ts"),
            callback: Resolution::Unknown { reason: "handler attached".to_string() },
        });
        let findings = AsyncMisuseDetector.detect(&project(vec![facts]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].span.start_byte, 20);
        assert_eq!(findings[0].evidence[0].code, "floating-known-async-call");
    }

    #[test]
    fn dead_surface_requires_literal_condition_and_placeholder_comment() {
        let mut facts = file("a.ts");
        facts.branches.extend([
            BranchFact {
                span: span("a.ts", 1, 10),
                condition_span: span("a.ts", 2, 3),
                scope: scope("a.ts"),
                condition: Some(ConstantCondition::AlwaysFalse),
                unreachable_span: Some(span("a.ts", 4, 9)),
                adjacent_placeholder_comment: Some(span("a.ts", 0, 1)),
            },
            BranchFact {
                span: span("a.ts", 20, 30),
                condition_span: span("a.ts", 21, 22),
                scope: scope("a.ts"),
                condition: Some(ConstantCondition::AlwaysFalse),
                unreachable_span: Some(span("a.ts", 23, 29)),
                adjacent_placeholder_comment: None,
            },
        ]);
        let findings = DeadSurfaceDetector.detect(&project(vec![facts]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence[0].code, "literal-placeholder-branch");
    }

    #[test]
    fn non_executing_test_reports_only_disabled_modes() {
        let mut facts = file("a.test.ts");
        for (start, mode) in [
            (1, TestMode::Skip),
            (10, TestMode::Todo),
            (20, TestMode::DisabledAlias),
            (30, TestMode::Only),
            (40, TestMode::Run),
        ] {
            facts.tests.push(TestFact {
                call_span: span("a.test.ts", start, start + 2),
                name: Some(format!("test-{start}")),
                mode,
                callback_span: None,
                callback_resolution_complete: false,
                assertion_spans: Vec::new(),
                mock_spans: Vec::new(),
                body_canonical: None,
                literal_vector: Vec::new(),
            });
        }
        let findings = NonExecutingTestDetector.detect(&project(vec![facts]));
        assert_eq!(findings.len(), 3);
        assert!(findings.iter().all(|finding| finding.evidence[0].code == "skipped-test"));
    }

    #[test]
    fn incomplete_and_generated_files_are_suppressed() {
        let mut incomplete = file("a.ts");
        incomplete.analysis_complete = false;
        incomplete.catches.push(catch("a.ts", 1, Vec::new()));
        let mut generated = file("generated.ts");
        generated.is_generated = true;
        generated.casts.push(cast("generated.ts", 1, CastKind::AsAny));
        let project = ProjectFacts {
            coverage: AnalysisCoverage {
                parse_incomplete_files: BTreeSet::from(["a.ts".to_string()]),
                ..AnalysisCoverage::default()
            },
            ..project(vec![incomplete, generated])
        };
        assert!(SwallowedFailureDetector.detect(&project).is_empty());
        assert!(SuppressionChainDetector.detect(&project).is_empty());
    }
}
