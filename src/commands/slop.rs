use std::collections::BTreeSet;
use std::fs;
use std::io;

use anyhow::{Context, Result, bail};

use crate::cli::SlopArgs;
use crate::scan::rules::is_test_path;
use crate::slop::diff::{detect_diff_findings, load_diff_scope};
use crate::slop::output::{SlopFilter, SlopOutputFormat, filter_and_sort_findings, write_report};
use crate::slop::types::{SlopKind, SlopOptions, SlopReport};
use crate::slop::{build_project_facts, collect_project_files, detectors};

pub fn run(args: &SlopArgs) -> Result<()> {
    let report = analyze(args)?;
    let format = if args.json { SlopOutputFormat::Json } else { SlopOutputFormat::Markdown };
    let stdout = io::stdout();
    write_report(&report, format, &mut stdout.lock())
}

fn analyze(args: &SlopArgs) -> Result<SlopReport> {
    let filter = validate_filter_args(args)?;
    let root = fs::canonicalize(&args.root).context("Cannot resolve project root")?;
    let diff = args.base.as_deref().map(|base| load_diff_scope(&root, base)).transpose()?;
    let files = collect_project_files(&root)?;
    let facts = build_project_facts(&root, files)?;
    let options = SlopOptions { include_test_files: args.include_test_files };
    let mut findings = detectors(&options)
        .into_iter()
        .flat_map(|detector| detector.detect(&facts))
        .collect::<Vec<_>>();
    if let Some(scope) = &diff {
        findings.extend(detect_diff_findings(&facts, scope));
    }
    findings.retain(|finding| {
        args.include_test_files
            || !is_test_path(&finding.span.path)
            || kind_requires_test_subject(finding.kind)
    });
    let findings = filter_and_sort_findings(findings, &filter);
    Ok(SlopReport {
        version: 1,
        root: root.to_string_lossy().to_string(),
        diff: diff.as_ref().map(crate::slop::diff::DiffScope::summary),
        findings,
    })
}

const fn kind_requires_test_subject(kind: SlopKind) -> bool {
    matches!(
        kind,
        SlopKind::NonExecutingTest
            | SlopKind::AssertionMonoculture
            | SlopKind::MockDominatedTest
            | SlopKind::DuplicatedTestBody
            | SlopKind::ImplementationMirroringTest
    )
}

fn validate_filter_args(args: &SlopArgs) -> Result<SlopFilter> {
    let valid = SlopKind::ALL.into_iter().map(SlopKind::cli_name).collect::<BTreeSet<_>>();
    let unknown = args
        .only
        .iter()
        .chain(&args.exclude)
        .filter(|name| !valid.contains(name.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();
    if !unknown.is_empty() {
        bail!(
            "unknown slop kind(s): {}; valid kinds: {}",
            unknown.into_iter().collect::<Vec<_>>().join(", "),
            valid.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(SlopFilter {
        minimum_confidence: Some(args.confidence),
        only: args.only.iter().filter_map(|name| SlopKind::from_cli_name(name)).collect(),
        exclude: args.exclude.iter().filter_map(|name| SlopKind::from_cli_name(name)).collect(),
        top: args.top,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::slop::types::SlopConfidence;

    use super::*;

    fn args() -> SlopArgs {
        SlopArgs {
            root: ".".to_string(),
            base: None,
            confidence: SlopConfidence::Medium,
            only: Vec::new(),
            exclude: Vec::new(),
            top: None,
            include_test_files: false,
            json: true,
        }
    }

    #[test]
    fn validates_known_names_and_reports_sorted_valid_names_before_scanning() {
        let mut input = args();
        input.root = "/path/that/does/not/exist".to_string();
        input.only.push("suppression-chain".to_string());
        input.exclude.push("patch-stack".to_string());
        validate_filter_args(&input).unwrap();

        input.only.push("unknown-kind".to_string());
        let error = analyze(&input).unwrap_err().to_string();
        assert!(error.contains("unknown-kind"));
        assert!(error.contains("assertion-monoculture"));
        assert!(error.contains("unresolved-api"));
        assert!(!error.contains("Cannot resolve project root"));
    }

    #[test]
    fn analyzes_without_a_git_repository_and_accepts_top_zero() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("index.ts"), "export const value = 1;\n").unwrap();
        let mut input = args();
        input.root = root.path().to_string_lossy().into_owned();
        input.top = Some(0);
        let report = analyze(&input).unwrap();
        assert_eq!(report.version, 1);
        assert_eq!(report.root, root.path().canonicalize().unwrap().to_string_lossy());
        assert!(report.diff.is_none());
        assert!(report.findings.is_empty());
    }

    #[test]
    fn ordinary_findings_exclude_test_paths_but_test_detectors_keep_them() {
        assert!(!kind_requires_test_subject(SlopKind::SuppressionChain));
        assert!(kind_requires_test_subject(SlopKind::NonExecutingTest));
        assert!(kind_requires_test_subject(SlopKind::AssertionMonoculture));
        assert!(is_test_path("src/value.test.ts"));
    }

    #[test]
    fn only_and_exclude_overlap_with_exclusion_winning() {
        let mut input = args();
        input.only = vec!["patch-stack".to_string()];
        input.exclude = vec!["patch-stack".to_string()];
        let filter = validate_filter_args(&input).unwrap();
        assert_eq!(filter.only, BTreeSet::from([SlopKind::PatchStack]));
        assert_eq!(filter.exclude, BTreeSet::from([SlopKind::PatchStack]));
    }
}
