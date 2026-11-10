//! `scanr deps`: what each file imports, resolved to project files, and what
//! imports it.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::DepsArgs;
use crate::dependencies::{DependencyGraph, FileDependencies};
use crate::slop::{build_project_facts, collect_project_files};

#[derive(Serialize)]
struct DepsFile<'a> {
    file: &'a str,
    #[serde(flatten)]
    dependencies: &'a FileDependencies,
}

#[derive(Serialize)]
struct DepsReport<'a> {
    version: u8,
    files: Vec<DepsFile<'a>>,
}

pub fn run(args: &DepsArgs) -> Result<()> {
    let root = Path::new(&args.root).canonicalize().context("Cannot resolve project root")?;
    let project = build_project_facts(&root, collect_project_files(&root)?)?;
    let graph = DependencyGraph::build(&project);
    let scope = args.path.as_deref().map(|path| path.trim_matches('/').to_string());
    let report = DepsReport {
        version: 1,
        files: graph
            .files
            .iter()
            .filter(|(file, _)| scope.as_deref().is_none_or(|scope| in_scope(file, scope)))
            .map(|(file, dependencies)| DepsFile { file, dependencies })
            .collect(),
    };

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

fn in_scope(file: &str, scope: &str) -> bool {
    scope.is_empty()
        || file == scope
        || file.strip_prefix(scope).is_some_and(|rest| rest.starts_with('/'))
}

fn render(report: &DepsReport<'_>) -> String {
    let mut out = String::from("# Dependencies\n");
    if report.files.is_empty() {
        out.push_str("\nNo files in scope.\n");
    }
    for file in &report.files {
        out.push('\n');
        out.push_str(file.file);
        out.push('\n');

        let imports = &file.dependencies.imports;
        let mut files: Vec<String> = imports
            .iter()
            .filter_map(|import| {
                let file = import.file.as_ref()?;
                Some(format!("{file}{}", if import.type_only { "  (type)" } else { "" }))
            })
            .collect();
        files.sort();
        files.dedup();
        section(&mut out, "imports", &files);

        let mut packages: Vec<&str> =
            imports.iter().filter_map(|import| import.package.as_deref()).collect();
        packages.sort_unstable();
        packages.dedup();
        if !packages.is_empty() {
            out.push_str(&format!("  packages     {}\n", packages.join(", ")));
        }

        let unresolved: Vec<&str> = imports
            .iter()
            .filter(|import| import.is_unresolved())
            .map(|import| import.source.as_str())
            .collect();
        if !unresolved.is_empty() {
            out.push_str(&format!("  unresolved   {}\n", unresolved.join(", ")));
        }

        let importers: Vec<String> = file.dependencies.imported_by.iter().cloned().collect();
        section(&mut out, "imported by", &importers);
    }
    out
}

fn section(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("  {title}\n"));
    for item in items {
        out.push_str(&format!("    {item}\n"));
    }
}

#[cfg(test)]
#[path = "deps_test.rs"]
mod tests;
