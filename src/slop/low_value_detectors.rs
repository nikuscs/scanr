//! Adapters for the balanced low-value scan rules.

use crate::scan::types::FunctionRole;
use crate::slop::types::{
    BodyShape, DeclarationFact, DeclarationKind, Detector, FileFacts, ProjectFacts, SlopConfidence,
    SlopEvidence, SlopFinding, SlopKind, SlopOptions,
};

const LOW_VALUE_MAX_LINES: u32 = 3;
const DOMINANT_CONTAINER_MIN_LINES: u32 = 300;
const DOMINANT_HELPER_MIN_COUNT: usize = 2;
const MAX_REFERENCES: usize = 2;

pub fn detectors(options: &SlopOptions) -> Vec<Box<dyn Detector>> {
    vec![
        Box::new(LowValueLocalHelper { include_test_files: options.include_test_files }),
        Box::new(DominantContainerTinyHelpers { include_test_files: options.include_test_files }),
    ]
}

struct LowValueLocalHelper {
    include_test_files: bool,
}

struct DominantContainerTinyHelpers {
    include_test_files: bool,
}

impl Detector for LowValueLocalHelper {
    fn kind(&self) -> SlopKind {
        SlopKind::LowValueLocalHelper
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            for declaration in file
                .declarations
                .iter()
                .filter(|declaration| low_value_helper(project, declaration))
            {
                let references = same_file_references(project, declaration);
                findings.push(SlopFinding {
                    kind: self.kind(),
                    confidence: self.confidence(),
                    span: declaration.span.clone(),
                    evidence: vec![SlopEvidence {
                        code: "balanced-low-value-helper".to_string(),
                        label: "small local helper".to_string(),
                        span: declaration.span.clone(),
                        detail: format!(
                            "`{}` is a {}-line top-level helper with {references} same-file reference(s) and a trivial body.",
                            declaration.key.name,
                            line_count(declaration)
                        ),
                    }],
                    explanation: "This small local helper has a trivial body and only one or two same-file references.".to_string(),
                    action: "Keep it when the name adds useful domain meaning; otherwise inline it at its local call sites.".to_string(),
                });
            }
        }
        sort_findings(&mut findings);
        findings
    }
}

impl Detector for DominantContainerTinyHelpers {
    fn kind(&self) -> SlopKind {
        SlopKind::DominantContainerTinyHelpers
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            let mut containers = file
                .declarations
                .iter()
                .filter(|declaration| {
                    matches!(
                        declaration.kind,
                        DeclarationKind::Function
                            | DeclarationKind::Method
                            | DeclarationKind::Class
                    ) && line_count(declaration) >= DOMINANT_CONTAINER_MIN_LINES
                })
                .collect::<Vec<_>>();
            containers.sort_by(|left, right| {
                line_count(right)
                    .cmp(&line_count(left))
                    .then_with(|| left.key.name.cmp(&right.key.name))
                    .then_with(|| left.span.cmp(&right.span))
            });
            let Some(container) = containers.first() else { continue };

            let mut helpers = file
                .declarations
                .iter()
                .filter(|declaration| tiny_local_helper(project, declaration))
                .collect::<Vec<_>>();
            helpers.sort_by(|left, right| left.span.cmp(&right.span));
            if helpers.len() < DOMINANT_HELPER_MIN_COUNT {
                continue;
            }

            let container_kind =
                if container.kind == DeclarationKind::Class { "class" } else { "function" };
            let mut evidence = vec![SlopEvidence {
                code: "dominant-container".to_string(),
                label: "dominant container".to_string(),
                span: container.span.clone(),
                detail: format!(
                    "The {container_kind} `{}` spans {} lines.",
                    container.key.name,
                    line_count(container)
                ),
            }];
            evidence.extend(helpers.into_iter().map(|helper| SlopEvidence {
                code: "tiny-low-use-helper".to_string(),
                label: "tiny local helper".to_string(),
                span: helper.span.clone(),
                detail: format!(
                    "`{}` spans {} line(s) and has {} same-file reference(s).",
                    helper.key.name,
                    line_count(helper),
                    same_file_references(project, helper)
                ),
            }));
            findings.push(SlopFinding {
                kind: self.kind(),
                confidence: self.confidence(),
                span: container.span.clone(),
                evidence,
                explanation: "A very large function or class sits beside several tiny, low-use local helpers.".to_string(),
                action: "Review the boundary as a whole: keep domain-significant helpers, and simplify or extract the dominant container where that improves ownership.".to_string(),
            });
        }
        sort_findings(&mut findings);
        findings
    }
}

const fn skipped(file: &FileFacts, include_test_files: bool) -> bool {
    !file.analysis_complete || file.is_generated || (!include_test_files && file.is_test)
}

fn low_value_helper(project: &ProjectFacts, declaration: &DeclarationFact) -> bool {
    tiny_local_helper(project, declaration)
        && declaration.body_shape != BodyShape::Other
        && line_count(declaration) <= LOW_VALUE_MAX_LINES
}

fn tiny_local_helper(project: &ProjectFacts, declaration: &DeclarationFact) -> bool {
    declaration.kind == DeclarationKind::Function
        && declaration.role == Some(FunctionRole::Helper)
        && declaration.exported_as.is_empty()
        && project
            .declaration_metadata
            .get(&declaration.key)
            .is_some_and(|metadata| metadata.top_level)
        && line_count(declaration) <= LOW_VALUE_MAX_LINES
        && (1..=MAX_REFERENCES).contains(&same_file_references(project, declaration))
}

fn same_file_references(project: &ProjectFacts, declaration: &DeclarationFact) -> usize {
    project
        .symbol_uses
        .get(&declaration.key)
        .map_or(0, |uses| uses.iter().filter(|span| span.path == declaration.key.path).count())
}

const fn line_count(declaration: &DeclarationFact) -> u32 {
    declaration.span.end_line.saturating_sub(declaration.span.start_line) + 1
}

fn sort_findings(findings: &mut [SlopFinding]) {
    findings.sort_by(|left, right| {
        left.span.cmp(&right.span).then_with(|| left.kind.sort_rank().cmp(&right.kind.sort_rank()))
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::slop::{build_project_facts, collect_project_files};

    fn facts(source: &str) -> ProjectFacts {
        let root = tempdir().unwrap();
        fs::write(root.path().join("helpers.ts"), source).unwrap();
        let root = root.path().canonicalize().unwrap();
        build_project_facts(&root, collect_project_files(&root).unwrap()).unwrap()
    }

    #[test]
    fn registry_matches_balanced_adapter_order() {
        let registry = detectors(&SlopOptions::default());
        assert_eq!(
            registry.iter().map(|detector| detector.kind()).collect::<Vec<_>>(),
            [SlopKind::LowValueLocalHelper, SlopKind::DominantContainerTinyHelpers]
        );
    }

    #[test]
    fn low_value_helper_requires_trivial_top_level_body_and_one_or_two_uses() {
        let project = facts(
            "function tiny(value: number) { return value; }\nexport function run() { return tiny(1); }\n",
        );
        let findings = LowValueLocalHelper { include_test_files: false }.detect(&project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].span.path, "helpers.ts");

        let project = facts(
            "function outer() { function nested(value: number) { return value; } return nested(1); }\nouter();\n",
        );
        assert!(LowValueLocalHelper { include_test_files: false }.detect(&project).is_empty());
    }

    #[test]
    fn dominant_container_groups_tiny_helpers_once() {
        let mut project = ProjectFacts::default();
        let span = |_name: &str, start: u32, end: u32| crate::slop::types::SourceSpan {
            path: "large.ts".to_string(),
            start_byte: start,
            end_byte: end,
            start_line: start,
            start_column: 1,
            end_line: end,
            end_column: 1,
        };
        let declaration = |name: &str, start: u32, end: u32, kind: DeclarationKind| {
            let key = crate::slop::types::SymbolKey {
                path: "large.ts".to_string(),
                declaration_start: start,
                name: name.to_string(),
            };
            DeclarationFact {
                key,
                span: span(name, start, end),
                body_span: None,
                scope: crate::slop::types::ScopeKey::Module("large.ts".to_string()),
                kind,
                exported_as: Vec::new(),
                ambient: false,
                has_body: true,
                is_async: false,
                is_generator: false,
                role: (kind == DeclarationKind::Function).then_some(FunctionRole::Helper),
                body_shape: BodyShape::Other,
                parameter_count: None,
                branch_complexity: 1,
                control_nesting: 0,
                await_spans: Vec::new(),
            }
        };
        let container = declaration("large", 1, 300, DeclarationKind::Class);
        let first = declaration("first", 310, 311, DeclarationKind::Function);
        let second = declaration("second", 320, 322, DeclarationKind::Function);
        for helper in [&first, &second] {
            project.declaration_metadata.insert(
                helper.key.clone(),
                crate::slop::types::DeclarationMetadata {
                    top_level: true,
                    reference_count_is_exact: true,
                    ..crate::slop::types::DeclarationMetadata::default()
                },
            );
            project.symbol_uses.insert(helper.key.clone(), vec![span("use", 400, 400)]);
        }
        project.files.insert(
            "large.ts".to_string(),
            FileFacts {
                path: "large.ts".to_string(),
                analysis_complete: true,
                declarations: vec![container, first, second],
                ..FileFacts::default()
            },
        );
        let findings = DominantContainerTinyHelpers { include_test_files: false }.detect(&project);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence.len(), 3);
        assert_eq!(BTreeMap::from([(findings[0].kind, 1)]).len(), 1);
    }
}
