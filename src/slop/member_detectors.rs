//! Members of factory-returned service objects that nothing reads: a name-based syntactic
//! detector, and the classification of the type-aware worker's resolved references.
use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Result, bail};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::commands::search::wildcard_match;
use crate::scan::rules::is_test_path;
use crate::slop::types::{
    Detector, FactoryMemberFact, FileFacts, ProjectFacts, SlopConfidence, SlopEvidence,
    SlopFinding, SlopKind, SlopOptions, SourceSpan,
};

pub fn detectors(options: &SlopOptions) -> Vec<Box<dyn Detector>> {
    vec![Box::new(UnusedFactoryMemberDetector { factories: options.member_factories.clone() })]
}

fn matches_factory(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|pattern| wildcard_match(name.as_bytes(), pattern.as_bytes()))
}

struct UnusedFactoryMemberDetector {
    factories: Vec<String>,
}

impl Detector for UnusedFactoryMemberDetector {
    fn kind(&self) -> SlopKind {
        SlopKind::UnusedFactoryMember
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let handed_off =
            project.files.values().flat_map(|file| &file.handed_off_calls).collect::<BTreeSet<_>>();
        let mut findings = Vec::new();
        for file in project.files.values() {
            if !file.analysis_complete || is_test_path(&file.path) || file.is_generated {
                continue;
            }
            let mut members = file
                .factory_members
                .iter()
                .filter(|member| {
                    matches_factory(&self.factories, &member.factory)
                        && !handed_off.contains(&member.factory)
                })
                .collect::<Vec<_>>();
            // A factory none of whose members is read by name is consumed as a whole
            // (iterated, spread or handed to a library), which names cannot follow.
            let read = members
                .iter()
                .filter(|member| accessed(project, file, member))
                .map(|member| member.factory.as_str())
                .collect::<BTreeSet<_>>();
            members.retain(|member| read.contains(member.factory.as_str()));
            let reported = topmost_dead(
                &members,
                |member| (member.factory.as_str(), member.path.as_str()),
                |member| !accessed(project, file, member),
                |_| 0,
            );
            for member in reported {
                findings.push(member_finding(
                    SlopKind::UnusedFactoryMember,
                    member.span.clone(),
                    format!(
                        "`{}` returns `{}`; no file reads a property named `{}` outside this declaration (0 accesses).",
                        member.factory, member.path, member.name
                    ),
                ));
            }
        }
        findings
    }
}

fn accessed(project: &ProjectFacts, owner: &FileFacts, member: &FactoryMemberFact) -> bool {
    project.files.values().any(|file| {
        file.property_accesses.get(&member.name).is_some_and(|starts| {
            file.path != owner.path
                || starts
                    .iter()
                    .any(|start| *start < member.span.start_byte || *start >= member.span.end_byte)
        })
    })
}

/// Keeps dead members whose ancestors are alive, in declaration order. A dead object whose
/// only child is dead with the same number of references reports that child, so
/// `profile: { remember }` surfaces `profile.remember`.
fn topmost_dead<'a, T>(
    members: &[&'a T],
    key: impl Fn(&T) -> (&str, &str),
    dead: impl Fn(&T) -> bool,
    references: impl Fn(&T) -> usize,
) -> Vec<&'a T> {
    let children = |parent: &T| {
        let (factory, path) = key(parent);
        members
            .iter()
            .copied()
            .filter(|member| {
                let (member_factory, member_path) = key(member);
                member_factory == factory
                    && member_path
                        .strip_prefix(path)
                        .and_then(|rest| rest.strip_prefix('.'))
                        .is_some_and(|rest| !rest.contains('.'))
            })
            .collect::<Vec<_>>()
    };
    let mut reported: Vec<(&str, &str)> = Vec::new();
    let mut result = Vec::new();
    for member in members {
        let (factory, path) = key(member);
        let covered = reported.iter().any(|(reported_factory, reported_path)| {
            *reported_factory == factory
                && path.strip_prefix(reported_path).is_some_and(|rest| rest.starts_with('.'))
        });
        if covered || !dead(member) {
            continue;
        }
        reported.push((factory, path));
        let mut chosen = *member;
        while let [child] = children(chosen).as_slice() {
            if !dead(child) || references(child) != references(chosen) {
                break;
            }
            chosen = child;
        }
        result.push(chosen);
    }
    result
}

fn member_finding(kind: SlopKind, span: SourceSpan, detail: String) -> SlopFinding {
    let (explanation, action) = match kind {
        SlopKind::TestOnlyFactoryMember => (
            "Only tests read this member of the factory's returned object.",
            "Remove the member and its implementation, or have tests use the real public path.",
        ),
        SlopKind::ExposureOnlyMember => (
            "Nothing reads this member, but the function it wraps is used through other wiring.",
            "Drop the member from the returned object and keep the function.",
        ),
        _ => (
            "Nothing reads this member of the factory's returned object.",
            "Remove the member, and its implementation when nothing else uses it.",
        ),
    };
    SlopFinding {
        kind,
        confidence: SlopConfidence::Medium,
        span: span.clone(),
        evidence: vec![SlopEvidence {
            code: kind.cli_name().to_string(),
            label: "factory member".to_string(),
            span,
            detail,
        }],
        explanation: explanation.to_string(),
        action: action.to_string(),
    }
}

#[derive(Debug, Serialize)]
struct MembersRequest {
    root: String,
    tsconfigs: Vec<String>,
    factories: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct MembersResponse {
    pub members: Vec<ResolvedMember>,
}

/// One returned member with every reference the TypeScript checker resolved to it, and the
/// references of the function it wraps outside the member (`wired`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedMember {
    pub file: String,
    pub factory: String,
    pub path: String,
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub wraps: bool,
    pub refs: Vec<ResolvedRef>,
    pub wired: Vec<ResolvedRef>,
    /// Where the factory's whole result is passed to a library-typed parameter.
    pub handed_off: Vec<ResolvedRef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolvedRef {
    pub file: String,
    pub line: u32,
    /// The reference sits in a type position (`Service['member']`, `typeof service.member`).
    #[serde(rename = "type")]
    pub type_only: bool,
}

/// Resolves factory members with the TypeScript checker (bun or node) and classifies them.
pub fn detect_type_aware(
    root: &Path,
    tsconfigs: &[String],
    factories: &[String],
) -> Result<Vec<SlopFinding>> {
    let tsconfigs = if tsconfigs.is_empty() {
        discover_tsconfigs(root)
    } else {
        tsconfigs
            .iter()
            .map(|path| {
                std::fs::canonicalize(path)
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| anyhow::anyhow!("tsconfig not found at {path}: {error}"))
            })
            .collect::<Result<_>>()?
    };
    if tsconfigs.is_empty() {
        bail!("--type-aware found no tsconfig.json under {}; pass --tsconfig", root.display());
    }
    let response: MembersResponse = crate::worker::run_worker(
        "members",
        &MembersRequest {
            root: root.to_string_lossy().into_owned(),
            tsconfigs,
            factories: factories.to_vec(),
        },
    )?;
    Ok(type_aware_findings(root, &response.members))
}

fn discover_tsconfigs(root: &Path) -> Vec<String> {
    let mut found = WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(false)
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name() == "tsconfig.json"
                && !entry.path().components().any(|part| part.as_os_str() == "node_modules")
        })
        .map(|entry| entry.path().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    found.sort();
    found
}

pub fn type_aware_findings(root: &Path, members: &[ResolvedMember]) -> Vec<SlopFinding> {
    let relative = |file: &str| {
        Path::new(file)
            .strip_prefix(root)
            .map_or_else(|_| file.to_string(), |path| path.to_string_lossy().replace('\\', "/"))
    };
    let production = |refs: &[ResolvedRef]| {
        refs.iter()
            .filter(|reference| !reference.type_only && !is_test_path(&relative(&reference.file)))
            .map(|reference| format!("{}:{}", relative(&reference.file), reference.line))
            .collect::<Vec<_>>()
    };
    let owned = members
        .iter()
        .filter(|member| {
            let file = relative(&member.file);
            Path::new(&member.file).starts_with(root)
                && !is_test_path(&file)
                && member.handed_off.iter().all(|at| is_test_path(&relative(&at.file)))
        })
        .collect::<Vec<_>>();
    let reported = topmost_dead(
        &owned,
        |member| (member.factory.as_str(), member.path.as_str()),
        |member| production(&member.refs).is_empty(),
        |member| member.refs.len(),
    );

    let mut findings = Vec::new();
    for member in reported {
        let file = relative(&member.file);
        // A member forwards to wired code when it (or every forwarding member below it) wraps
        // a function that production code also reaches without this member.
        let wrappers = owned
            .iter()
            .filter(|other| {
                other.file == member.file
                    && other.factory == member.factory
                    && other.wraps
                    && (other.path == member.path
                        || other.path.starts_with(&format!("{}.", member.path)))
            })
            .collect::<Vec<_>>();
        let wired = wrappers.iter().flat_map(|other| production(&other.wired)).collect::<Vec<_>>();
        let tests = member
            .refs
            .iter()
            .filter(|reference| is_test_path(&relative(&reference.file)))
            .map(|reference| format!("{}:{}", relative(&reference.file), reference.line))
            .collect::<Vec<_>>();
        let types = member.refs.iter().filter(|reference| reference.type_only).count();
        let subject =
            format!("`{}` returns `{}`; 0 production references", member.factory, member.path);
        let type_note = if types == 0 { String::new() } else { format!(" ({types} type-only)") };
        let (kind, detail) = if !wrappers.is_empty()
            && wrappers.iter().all(|other| !production(&other.wired).is_empty())
        {
            (
                SlopKind::ExposureOnlyMember,
                format!(
                    "{subject}{type_note}, {} test; the wrapped function has {} elsewhere: {}.",
                    tests.len(),
                    plural(wired.len(), "production reference"),
                    sample(&wired)
                ),
            )
        } else if tests.is_empty() {
            (SlopKind::UnusedFactoryMember, format!("{subject}{type_note}, 0 test."))
        } else {
            (
                SlopKind::TestOnlyFactoryMember,
                format!("{subject}{type_note}, {} test: {}.", tests.len(), sample(&tests)),
            )
        };
        let span = SourceSpan {
            path: file,
            start_byte: member.start_byte,
            end_byte: member.end_byte,
            start_line: member.start_line,
            start_column: member.start_column,
            end_line: member.end_line,
            end_column: member.end_column,
        };
        findings.push(member_finding(kind, span, detail));
    }
    findings
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

fn sample(locations: &[String]) -> String {
    let mut shown = locations.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    if locations.len() > 3 {
        shown.push_str(&format!(" (+{} more)", locations.len() - 3));
    }
    shown
}
