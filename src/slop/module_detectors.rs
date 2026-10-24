//! File-level forwarding and single-consumer review signals.
use crate::slop::types::{
    Detector, ModuleShape, ProjectFacts, SlopConfidence, SlopEvidence, SlopFinding, SlopKind,
    SlopOptions,
};

pub fn detectors(options: &SlopOptions) -> Vec<Box<dyn Detector>> {
    [SlopKind::PassThroughModule, SlopKind::OversplitModule]
        .into_iter()
        .map(|kind| {
            Box::new(ModuleDetector { kind, options: options.clone() }) as Box<dyn Detector>
        })
        .collect()
}

struct ModuleDetector {
    kind: SlopKind,
    options: SlopOptions,
}

impl Detector for ModuleDetector {
    fn kind(&self) -> SlopKind {
        self.kind
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in project.files.values() {
            if !file.analysis_complete
                || file.is_test
                || file.is_generated
                || project.entrypoints.contains(&file.path)
            {
                continue;
            }
            let Some(span) = &file.module_span else {
                continue;
            };
            let detail = if self.kind == SlopKind::PassThroughModule {
                if file.module_shape != ModuleShape::PassThrough {
                    continue;
                }
                "All exports re-export imports or forward unchanged arguments without local logic."
                    .to_string()
            } else {
                if span.end_line > self.options.oversplit_max_lines
                    || file.module_shape == ModuleShape::NoExports
                {
                    continue;
                }
                let Some(importers) = project.module_importers.get(&file.path) else {
                    continue;
                };
                if importers.len() != 1 {
                    continue;
                }
                let importer = importers.first().expect("one importer");
                if self.options.oversplit_allow_importer.matched(importer, false).is_ignore() {
                    continue;
                }
                format!(
                    "{} physical lines; single analyzed non-test importer: `{importer}`.",
                    span.end_line
                )
            };
            findings.push(SlopFinding {
                kind: self.kind(),
                confidence: self.confidence(),
                span: span.clone(),
                evidence: vec![SlopEvidence {
                    code: self.kind.cli_name().to_string(),
                    label: "module boundary".to_string(),
                    span: span.clone(),
                    detail,
                }],
                explanation: if self.kind == SlopKind::PassThroughModule {
                    "This module adds a forwarding boundary without its own logic."
                } else {
                    "This small module has only one analyzed non-test consumer."
                }.to_string(),
                action: "Keep intentional public or domain boundaries; otherwise consider merging into the consumer.".to_string(),
            });
        }
        findings
    }
}
