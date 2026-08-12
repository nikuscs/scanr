use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::io::Write;

use anyhow::Result;

use crate::slop::types::{
    SlopConfidence, SlopEvidence, SlopFinding, SlopKind, SlopReport, SourceSpan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlopOutputFormat {
    Markdown,
    Json,
}

#[derive(Debug, Clone, Default)]
pub struct SlopFilter {
    pub minimum_confidence: Option<SlopConfidence>,
    pub only: BTreeSet<SlopKind>,
    pub exclude: BTreeSet<SlopKind>,
    pub top: Option<usize>,
}

pub fn filter_and_sort_findings(
    mut findings: Vec<SlopFinding>,
    filter: &SlopFilter,
) -> Vec<SlopFinding> {
    for finding in &mut findings {
        finding.evidence.sort_by(compare_evidence);
        finding.evidence.dedup();
    }
    findings.retain(|finding| {
        (filter.only.is_empty() || filter.only.contains(&finding.kind))
            && !filter.exclude.contains(&finding.kind)
            && filter.minimum_confidence.is_none_or(|minimum| {
                confidence_rank(finding.confidence) <= confidence_rank(minimum)
            })
    });
    findings.sort_by(compare_findings);
    let low_value_spans = findings
        .iter()
        .filter(|finding| finding.kind == SlopKind::LowValueLocalHelper)
        .map(|finding| finding.span.clone())
        .collect::<BTreeSet<_>>();
    findings.retain(|finding| {
        finding.kind != SlopKind::OneUseAbstraction || !low_value_spans.contains(&finding.span)
    });
    if let Some(top) = filter.top {
        findings.truncate(top);
    }
    findings
}

pub fn write_report<W: Write>(
    report: &SlopReport,
    format: SlopOutputFormat,
    writer: &mut W,
) -> Result<()> {
    if format == SlopOutputFormat::Json {
        serde_json::to_writer(&mut *writer, report)?;
        writer.write_all(b"\n")?;
        return Ok(());
    }

    writer.write_all(
        b"| File | Finding | Confidence | Evidence | Why it matters | Suggested action |\n\
| --- | --- | --- | --- | --- | --- |\n",
    )?;
    for finding in &report.findings {
        let evidence = finding
            .evidence
            .iter()
            .map(|item| format!("{} at {}: {}", item.label, display_span(&item.span), item.detail))
            .collect::<Vec<_>>()
            .join("; ");
        writeln!(
            writer,
            "| {} | {} | {} | {} | {} | {} |",
            escape_markdown(&display_span(&finding.span)),
            escape_markdown(finding.kind.display_name()),
            escape_markdown(confidence_name(finding.confidence)),
            escape_markdown(&evidence),
            escape_markdown(&finding.explanation),
            escape_markdown(&finding.action),
        )?;
    }
    Ok(())
}

fn compare_findings(left: &SlopFinding, right: &SlopFinding) -> Ordering {
    span_key(&left.span)
        .cmp(&span_key(&right.span))
        .then_with(|| left.kind.sort_rank().cmp(&right.kind.sort_rank()))
        .then_with(|| confidence_rank(left.confidence).cmp(&confidence_rank(right.confidence)))
        .then_with(|| compare_evidence_slices(&left.evidence, &right.evidence))
        .then_with(|| left.explanation.cmp(&right.explanation))
        .then_with(|| left.action.cmp(&right.action))
}

fn compare_evidence(left: &SlopEvidence, right: &SlopEvidence) -> Ordering {
    span_key(&left.span)
        .cmp(&span_key(&right.span))
        .then_with(|| left.label.cmp(&right.label))
        .then_with(|| left.detail.cmp(&right.detail))
        .then_with(|| left.code.cmp(&right.code))
}

fn compare_evidence_slices(left: &[SlopEvidence], right: &[SlopEvidence]) -> Ordering {
    for (left, right) in left.iter().zip(right) {
        let ordering = compare_evidence(left, right);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn span_key(span: &SourceSpan) -> (&str, u32, u32, u32, u32) {
    (&span.path, span.start_line, span.start_column, span.end_line, span.end_column)
}

const fn confidence_rank(confidence: SlopConfidence) -> u8 {
    match confidence {
        SlopConfidence::High => 0,
        SlopConfidence::Medium => 1,
    }
}

const fn confidence_name(confidence: SlopConfidence) -> &'static str {
    match confidence {
        SlopConfidence::High => "High",
        SlopConfidence::Medium => "Medium",
    }
}

fn display_span(span: &SourceSpan) -> String {
    if span.start_line == span.end_line {
        format!("{}:{}", span.path, span.start_line)
    } else {
        format!("{}:{}-{}", span.path, span.start_line, span.end_line)
    }
}

fn escape_markdown(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(path: &str, start_line: u32, end_line: u32) -> SourceSpan {
        SourceSpan {
            path: path.to_string(),
            start_byte: 0,
            end_byte: 1,
            start_line,
            start_column: 1,
            end_line,
            end_column: 2,
        }
    }

    fn finding(kind: SlopKind, confidence: SlopConfidence, path: &str, line: u32) -> SlopFinding {
        SlopFinding {
            kind,
            confidence,
            span: span(path, line, line),
            evidence: vec![SlopEvidence {
                code: "evidence".to_string(),
                label: "signal".to_string(),
                span: span(path, line, line),
                detail: "detail".to_string(),
            }],
            explanation: "why".to_string(),
            action: "act".to_string(),
        }
    }

    #[test]
    fn filters_only_exclude_confidence_and_top_after_sorting() {
        let findings = vec![
            finding(SlopKind::PatchStack, SlopConfidence::Medium, "b.ts", 2),
            finding(SlopKind::SwallowedFailure, SlopConfidence::High, "a.ts", 3),
            finding(SlopKind::SuppressionChain, SlopConfidence::High, "a.ts", 1),
        ];
        let filter = SlopFilter {
            minimum_confidence: Some(SlopConfidence::High),
            only: BTreeSet::from([
                SlopKind::SuppressionChain,
                SlopKind::SwallowedFailure,
                SlopKind::PatchStack,
            ]),
            exclude: BTreeSet::from([SlopKind::SwallowedFailure]),
            top: Some(1),
        };
        let result = filter_and_sort_findings(findings, &filter);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, SlopKind::SuppressionChain);

        let empty = SlopFilter { top: Some(0), ..SlopFilter::default() };
        assert!(filter_and_sort_findings(result, &empty).is_empty());
    }

    #[test]
    fn overlapping_low_value_and_one_use_findings_collapse_to_one_row() {
        let findings = vec![
            finding(SlopKind::OneUseAbstraction, SlopConfidence::Medium, "a.ts", 3),
            finding(SlopKind::LowValueLocalHelper, SlopConfidence::Medium, "a.ts", 3),
        ];
        let filtered = filter_and_sort_findings(findings, &SlopFilter::default());
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].kind, SlopKind::LowValueLocalHelper);
    }

    #[test]
    fn evidence_is_sorted_deduplicated_and_input_order_independent() {
        let mut first = finding(SlopKind::PatchStack, SlopConfidence::Medium, "a.ts", 1);
        let duplicate = first.evidence[0].clone();
        first.evidence.insert(
            0,
            SlopEvidence {
                code: "later".to_string(),
                label: "later".to_string(),
                span: span("z.ts", 4, 4),
                detail: "later".to_string(),
            },
        );
        first.evidence.push(duplicate);
        let second = finding(SlopKind::SwallowedFailure, SlopConfidence::High, "b.ts", 1);

        let left =
            filter_and_sort_findings(vec![second.clone(), first.clone()], &SlopFilter::default());
        let right = filter_and_sort_findings(vec![first, second], &SlopFilter::default());
        assert_eq!(left, right);
        assert_eq!(left[0].evidence.len(), 2);
        assert_eq!(left[0].evidence[0].span.path, "a.ts");
    }

    #[test]
    fn markdown_has_one_escaped_row_with_full_line_range() {
        let mut item = finding(SlopKind::PatchStack, SlopConfidence::Medium, "src/a|b.ts", 2);
        item.span.end_line = 5;
        item.evidence[0].label = "path\\signal".to_string();
        item.evidence[0].detail = "first\r\nsecond|third".to_string();
        item.explanation = "plain\nEnglish".to_string();
        let report = SlopReport {
            version: 1,
            root: "/project".to_string(),
            diff: None,
            findings: vec![item],
        };
        let mut bytes = Vec::new();
        write_report(&report, SlopOutputFormat::Markdown, &mut bytes).unwrap();
        let output = String::from_utf8(bytes).unwrap();
        assert_eq!(output.lines().count(), 3);
        assert!(output.contains("src/a\\|b.ts:2-5"));
        assert!(output.contains("path\\\\signal"));
        assert!(output.contains("first<br>second\\|third"));
        assert!(!output.contains("PatchStack"));
    }

    #[test]
    fn empty_markdown_is_exact_and_newline_terminated() {
        let report = SlopReport {
            version: 1,
            root: "/project".to_string(),
            diff: None,
            findings: Vec::new(),
        };
        let mut bytes = Vec::new();
        write_report(&report, SlopOutputFormat::Markdown, &mut bytes).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "| File | Finding | Confidence | Evidence | Why it matters | Suggested action |\n| --- | --- | --- | --- | --- | --- |\n"
        );
    }

    #[test]
    fn json_report_is_versioned_compact_and_newline_terminated() {
        let report = SlopReport {
            version: 1,
            root: "/project".to_string(),
            diff: None,
            findings: Vec::new(),
        };
        let mut bytes = Vec::new();
        write_report(&report, SlopOutputFormat::Json, &mut bytes).unwrap();
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            "{\"version\":1,\"root\":\"/project\",\"findings\":[]}\n"
        );
        let _: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    }

    #[test]
    fn repeated_sort_and_render_is_byte_identical_for_both_formats() {
        let first = finding(SlopKind::PatchStack, SlopConfidence::Medium, "b.ts", 2);
        let second = finding(SlopKind::SwallowedFailure, SlopConfidence::High, "a.ts", 1);
        let render = |input: Vec<SlopFinding>, format| {
            let report = SlopReport {
                version: 1,
                root: "/project".to_string(),
                diff: Some(crate::slop::types::DiffSummary {
                    requested_base: "HEAD~1".to_string(),
                    resolved_base: "0123456789abcdef0123456789abcdef01234567".to_string(),
                    changed_files: 2,
                    added_lines: 4,
                }),
                findings: filter_and_sort_findings(input, &SlopFilter::default()),
            };
            let mut bytes = Vec::new();
            write_report(&report, format, &mut bytes).unwrap();
            bytes
        };
        for format in [SlopOutputFormat::Markdown, SlopOutputFormat::Json] {
            assert_eq!(
                render(vec![first.clone(), second.clone()], format),
                render(vec![second.clone(), first.clone()], format)
            );
        }
    }
}
