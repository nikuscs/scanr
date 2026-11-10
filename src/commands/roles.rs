//! `scanr roles`: groups files by name suffix (`*.layer.ts`, `*.fake.ts`) and
//! lists the files that break the pattern their group follows.
//!
//! A file's traits are what it declares at the top level and which roles it
//! imports. A trait shared by most of a role is usual; a file without it, or
//! with a trait almost no sibling has, is an odd one out.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::RolesArgs;
use crate::commands::tree::parse_project;
use crate::dependencies::{DependencyGraph, FileDependencies};
use crate::scan::outline::FileOutline;
use crate::scan::rules::is_test_path;
use crate::slop::build_project_facts;

const PLAIN_ROLE: &str = "(no suffix)";
const TEST_ROLE: &str = "test";

#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub min_files: usize,
    /// Percent of a role's files that makes a trait usual.
    pub usual: u8,
    /// Percent of a role's files at or below which a trait is unusual.
    pub unusual: u8,
}

/// One file's role and traits. `distinctive` traits may be reported as unusual;
/// names and producers vary too freely for their rarity to mean anything.
pub struct RoleFile {
    pub path: String,
    pub role: String,
    pub traits: BTreeMap<String, bool>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TraitCount {
    #[serde(rename = "trait")]
    pub name: String,
    pub files: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Outlier {
    pub file: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unusual: Vec<TraitCount>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Role {
    #[serde(rename = "role")]
    pub name: String,
    pub files: usize,
    pub usual: Vec<TraitCount>,
    pub outliers: Vec<Outlier>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SmallRole {
    #[serde(rename = "role")]
    pub name: String,
    pub files: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolesReport {
    pub version: u8,
    pub roles: Vec<Role>,
    /// Roles with too few files to have a pattern. A one-off suffix can itself be drift.
    pub small_roles: Vec<SmallRole>,
}

pub fn run(args: &RolesArgs) -> Result<()> {
    let root = Path::new(&args.root).canonicalize().context("Cannot resolve project root")?;
    let (indices, similarity) = parse_project(&root, &root, true)?;
    let project = build_project_facts(&root, similarity)?;
    let graph = DependencyGraph::build(&project);

    let files: Vec<RoleFile> = indices
        .iter()
        .filter(|index| !is_test_path(&index.path))
        .map(|index| RoleFile {
            path: index.path.clone(),
            role: file_role(&index.path),
            traits: file_traits(&index.outline, graph.files.get(&index.path)),
        })
        .collect();
    let thresholds =
        Thresholds { min_files: args.min_files, usual: args.usual, unusual: args.unusual };
    let report = build_report(&files, thresholds);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if args.json {
        serde_json::to_writer(&mut out, &report)?;
        writeln!(out)?;
    } else {
        write!(out, "{}", render(&report))?;
    }
    Ok(())
}

/// `events.layer.ts` → `.layer`, `index.ts` → `index`, `events.ts` → `(no suffix)`.
pub fn file_role(path: &str) -> String {
    if is_test_path(path) {
        return TEST_ROLE.to_string();
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.strip_suffix(".d.ts").or_else(|| name.rsplit_once('.').map(|(stem, _)| stem));
    match stem.unwrap_or(name).rsplit_once('.') {
        Some((_, suffix)) => format!(".{suffix}"),
        None if stem == Some("index") => "index".to_string(),
        None => PLAIN_ROLE.to_string(),
    }
}

/// Top-level traits of a file: each declared name, each kind of declaration,
/// each producer, and each role it imports.
pub fn file_traits(
    outline: &FileOutline,
    dependencies: Option<&FileDependencies>,
) -> BTreeMap<String, bool> {
    let mut traits = BTreeMap::new();
    for symbol in &outline.symbols {
        let visibility = if symbol.exported { "exported" } else { "internal" };
        let kind = symbol.kind.as_str();
        traits.insert(
            if symbol.exported {
                format!("exported `{}`", symbol.name)
            } else {
                format!("`{}`", symbol.name)
            },
            false,
        );
        traits.insert(format!("{visibility} {kind}"), true);
        if let Some(via) = &symbol.via {
            traits.insert(format!("{visibility} {kind} = {via}"), false);
        }
    }
    if !outline.reexports.is_empty() {
        traits.insert("re-exports".to_string(), true);
    }
    for file in dependencies.into_iter().flat_map(|d| &d.imports).filter_map(|i| i.file.as_ref()) {
        traits.insert(format!("imports {}", file_role(file)), true);
    }
    traits
}

pub fn build_report(files: &[RoleFile], thresholds: Thresholds) -> RolesReport {
    let mut groups: BTreeMap<&str, Vec<&RoleFile>> = BTreeMap::new();
    for file in files {
        groups.entry(&file.role).or_default().push(file);
    }

    let mut roles = Vec::new();
    let mut small_roles = Vec::new();
    for (role, members) in groups {
        if members.len() < thresholds.min_files {
            small_roles.push(SmallRole {
                name: role.to_string(),
                files: members.iter().map(|file| file.path.clone()).collect(),
            });
        } else {
            roles.push(role_report(role, &members, thresholds));
        }
    }
    roles.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.name.cmp(&b.name)));
    RolesReport { version: 1, roles, small_roles }
}

fn role_report(role: &str, members: &[&RoleFile], thresholds: Thresholds) -> Role {
    let total = members.len();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for file in members {
        for name in file.traits.keys() {
            *counts.entry(name).or_default() += 1;
        }
    }
    let is_usual = |count: usize| count * 100 >= total * usize::from(thresholds.usual);
    let is_unusual = |count: usize| count * 100 <= total * usize::from(thresholds.unusual);

    let mut usual: Vec<TraitCount> = counts
        .iter()
        .filter(|(_, count)| is_usual(**count))
        .map(|(name, count)| TraitCount { name: (*name).to_string(), files: *count })
        .collect();
    usual.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.name.cmp(&b.name)));
    let expected: BTreeSet<&str> = usual.iter().map(|usual| usual.name.as_str()).collect();

    // A group that shares nothing has no pattern for a file to break.
    let compared: &[&RoleFile] = if usual.is_empty() { &[] } else { members };
    let mut outliers: Vec<Outlier> = compared
        .iter()
        .filter_map(|file| {
            let missing: Vec<String> = expected
                .iter()
                .filter(|name| !file.traits.contains_key(**name))
                .map(|name| (*name).to_string())
                .collect();
            let unusual: Vec<TraitCount> = file
                .traits
                .iter()
                .filter(|(name, distinctive)| **distinctive && is_unusual(counts[name.as_str()]))
                .map(|(name, _)| TraitCount { name: name.clone(), files: counts[name.as_str()] })
                .collect();
            (!missing.is_empty() || !unusual.is_empty()).then(|| Outlier {
                file: file.path.clone(),
                missing,
                unusual,
            })
        })
        .collect();
    outliers.sort_by(|a, b| a.file.cmp(&b.file));

    Role { name: role.to_string(), files: total, usual, outliers }
}

fn render(report: &RolesReport) -> String {
    let mut out = String::from("# File roles\n");
    if report.roles.is_empty() {
        out.push_str("\nNo role has enough files to compare.\n");
    }
    for role in &report.roles {
        out.push_str(&format!("\n## {} — {} files\n", role.name, role.files));
        if role.usual.is_empty() {
            out.push_str("No shared pattern.\n");
        } else {
            let usual: Vec<String> = role
                .usual
                .iter()
                .map(|usual| format!("{} ({})", usual.name, usual.files))
                .collect();
            out.push_str(&format!("Usually: {}\n", usual.join(", ")));
        }
        if role.outliers.is_empty() {
            out.push_str("No odd ones out.\n");
            continue;
        }
        out.push_str("Odd ones out:\n");
        for outlier in &role.outliers {
            out.push_str(&format!("- {}\n", outlier.file));
            if !outlier.missing.is_empty() {
                out.push_str(&format!("    missing  {}\n", outlier.missing.join(", ")));
            }
            if !outlier.unusual.is_empty() {
                let unusual: Vec<String> = outlier
                    .unusual
                    .iter()
                    .map(|unusual| {
                        format!("{} ({} of {})", unusual.name, unusual.files, role.files)
                    })
                    .collect();
                out.push_str(&format!("    unusual  {}\n", unusual.join(", ")));
            }
        }
    }
    if !report.small_roles.is_empty() {
        out.push_str("\n## Roles with too few files to compare\n");
        for role in &report.small_roles {
            out.push_str(&format!("- {} — {}\n", role.name, role.files.join(", ")));
        }
    }
    out
}

#[cfg(test)]
#[path = "roles_test.rs"]
mod tests;
