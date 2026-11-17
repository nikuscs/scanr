use std::collections::BTreeSet;
use std::fs;
use std::io;

use anyhow::{Context, Result, bail};

use crate::cli::SlopArgs;
use crate::scan::rules::is_test_path;
use crate::slop::diff::{detect_diff_findings, load_diff_scope};
use crate::slop::leftovers::detect_leftovers;
use crate::slop::member_detectors::detect_type_aware;
use crate::slop::output::{SlopFilter, SlopOutputFormat, filter_and_sort_findings, write_report};
use crate::slop::types::{SlopKind, SlopOptions, SlopReport};
use crate::slop::{build_project_facts, collect_project_files, detectors};

pub fn run(args: &SlopArgs) -> Result<()> {
    let report = analyze(args)?;
    let format = if args.json { SlopOutputFormat::Json } else { SlopOutputFormat::Markdown };
    let stdout = io::stdout();
    write_report(&report, format, &mut stdout.lock())
}

pub fn analyze(args: &SlopArgs) -> Result<SlopReport> {
    let filter = validate_filter_args(args)?;
    let mut allowed = ignore::gitignore::GitignoreBuilder::new("");
    for pattern in &args.oversplit_allow_importer {
        allowed.add_line(None, pattern).context("Invalid oversplit importer glob")?;
    }
    let options = SlopOptions {
        include_test_files: args.include_test_files,
        oversplit_max_lines: args.oversplit_max_lines,
        oversplit_allow_importer: allowed.build()?,
        member_factories: args.member_factory.clone(),
    };
    let root = fs::canonicalize(&args.root).context("Cannot resolve project root")?;
    let diff = args.base.as_deref().map(|base| load_diff_scope(&root, base)).transpose()?;
    let files = collect_project_files(&root)?;
    let facts = build_project_facts(&root, files)?;
    let mut findings = detectors(&options)
        .into_iter()
        .flat_map(|detector| detector.detect(&facts))
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    if let (Some(scope), Some(base)) = (&diff, args.base.as_deref()) {
        findings.extend(detect_diff_findings(&facts, scope));
        match detect_leftovers(&root, base, &facts) {
            Ok(leftovers) => findings.extend(leftovers),
            Err(error) => warnings.push(format!("leftover-reference skipped: {error:#}")),
        }
    }
    if args.type_aware {
        let type_aware = detect_type_aware(&root, &args.tsconfig, &args.member_factory)?;
        warnings = type_aware.warnings;
        if let Some(type_aware) = type_aware.findings {
            findings.retain(|finding| finding.kind != SlopKind::UnusedFactoryMember);
            findings.extend(type_aware);
        }
    }
    if args.changed_only
        && let Some(scope) = &diff
    {
        findings.retain(|finding| {
            finding.kind.is_diff_only()
                || scope.intersects(&finding.span)
                || finding.evidence.iter().any(|row| scope.intersects(&row.span))
        });
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
        warnings,
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
        .filter(|name| SlopKind::from_cli_name(name).is_none())
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
            changed_only: false,
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
        }
    }

    #[test]
    fn module_kinds_accept_camel_case_and_canonical_names() {
        let mut input = args();
        input.only = vec!["passThroughModule".to_string(), "oversplitModule".to_string()];
        input.exclude = vec!["oversplit-module".to_string()];
        let filter = validate_filter_args(&input).unwrap();
        assert_eq!(
            filter.only,
            BTreeSet::from([SlopKind::PassThroughModule, SlopKind::OversplitModule])
        );
        assert_eq!(filter.exclude, BTreeSet::from([SlopKind::OversplitModule]));
        input.oversplit_allow_importer = vec!["{a,b".to_string()];
        assert!(
            analyze(&input).unwrap_err().to_string().contains("Invalid oversplit importer glob")
        );
    }

    #[test]
    fn member_kinds_accept_camel_case_and_canonical_names() {
        let mut input = args();
        input.only = vec![
            "unusedFactoryMember".to_string(),
            "testOnlyFactoryMember".to_string(),
            "exposure-only-member".to_string(),
        ];
        let filter = validate_filter_args(&input).unwrap();
        assert_eq!(
            filter.only,
            BTreeSet::from([
                SlopKind::UnusedFactoryMember,
                SlopKind::TestOnlyFactoryMember,
                SlopKind::ExposureOnlyMember
            ])
        );
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
        assert_eq!(report.findings, [] as [crate::slop::types::SlopFinding; 0]);
    }

    #[test]
    fn ordinary_findings_exclude_test_paths_but_test_detectors_keep_them() {
        assert!(!kind_requires_test_subject(SlopKind::SuppressionChain));
        assert!(kind_requires_test_subject(SlopKind::NonExecutingTest));
        assert!(kind_requires_test_subject(SlopKind::AssertionMonoculture));
        assert!(is_test_path("src/value.test.ts"));
    }

    #[test]
    fn changed_only_keeps_findings_on_added_lines() {
        let root = tempdir().unwrap();
        let path = root.path();
        let git = |args: &[&str]| {
            let status =
                std::process::Command::new("git").args(args).current_dir(path).status().unwrap();
            assert!(status.success(), "git {args:?}");
        };
        let old = "export interface Old { a: string; b: string; c: string }\n";
        let new = "export interface Fresh { x: number; y: number; z: number }\n";
        git(&["init", "-q"]);
        git(&["config", "user.email", "scanr@example.invalid"]);
        git(&["config", "user.name", "scanr test"]);
        fs::write(path.join("a.ts"), old).unwrap();
        fs::write(path.join("b.ts"), old).unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        fs::write(path.join("c.ts"), new).unwrap();
        fs::write(path.join("d.ts"), new).unwrap();

        let mut input = args();
        input.root = path.to_string_lossy().into_owned();
        input.base = Some("HEAD".to_string());
        input.only = vec!["copied-type".to_string()];
        assert_eq!(analyze(&input).unwrap().findings.len(), 2);
        input.changed_only = true;
        let findings = analyze(&input).unwrap().findings;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence[0].code, "Fresh");
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
