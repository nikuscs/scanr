use std::io::{self, Write};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::RefsArgs;
use crate::slop::types::{DeclarationKind, ProjectFacts};
use crate::slop::{build_project_facts, collect_project_files};

#[derive(Debug, Serialize)]
struct RefsReport {
    ver: u8,
    name: String,
    matches: Vec<RefsMatch>,
}

#[derive(Debug, Serialize)]
struct RefsMatch {
    declaration: RefsDeclaration,
    usages: Vec<RefsUsage>,
}

#[derive(Debug, Serialize)]
struct RefsDeclaration {
    file: String,
    line: u32,
    kind: String,
    exported: bool,
}

#[derive(Debug, Serialize)]
struct RefsUsage {
    file: String,
    line: u32,
}

pub fn run(args: &RefsArgs) -> Result<()> {
    let root = std::fs::canonicalize(&args.root).context("Cannot resolve project root")?;
    let files = collect_project_files(&root)?;
    let project = build_project_facts(&root, files)?;
    let report = lookup(&project, &args.name);
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, &report)?;
    handle.write_all(b"\n")?;
    Ok(())
}

fn lookup(project: &ProjectFacts, name: &str) -> RefsReport {
    let mut matches = Vec::new();
    for facts in project.files.values() {
        for declaration in &facts.declarations {
            let name_hit = declaration.key.name == name
                || declaration.exported_as.iter().any(|exported| exported == name);
            if !name_hit {
                continue;
            }
            let usages = project
                .symbol_uses
                .get(&declaration.key)
                .into_iter()
                .flatten()
                .filter(|span| {
                    !(span.path == declaration.span.path
                        && span.start_line == declaration.span.start_line)
                })
                .map(|span| RefsUsage { file: span.path.clone(), line: span.start_line })
                .collect::<Vec<_>>();
            matches.push(RefsMatch {
                declaration: RefsDeclaration {
                    file: declaration.span.path.clone(),
                    line: declaration.span.start_line,
                    kind: declaration_kind_name(declaration.kind).to_string(),
                    exported: !declaration.exported_as.is_empty(),
                },
                usages,
            });
        }
    }
    for item in &mut matches {
        item.usages.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
        item.usages.dedup_by(|a, b| a.file == b.file && a.line == b.line);
    }
    matches.sort_by(|a, b| {
        a.declaration
            .file
            .cmp(&b.declaration.file)
            .then(a.declaration.line.cmp(&b.declaration.line))
    });
    RefsReport { ver: 1, name: name.to_string(), matches }
}

const fn declaration_kind_name(kind: DeclarationKind) -> &'static str {
    match kind {
        DeclarationKind::Function => "function",
        DeclarationKind::Method => "method",
        DeclarationKind::Variable => "variable",
        DeclarationKind::Class => "class",
        DeclarationKind::Interface => "interface",
        DeclarationKind::TypeAlias => "type",
        DeclarationKind::Enum => "enum",
        DeclarationKind::Property => "property",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn project_from(files: &[(&str, &str)]) -> ProjectFacts {
        let root = tempdir().unwrap();
        for (path, source) in files {
            let dest = root.path().join(path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(dest, source).unwrap();
        }
        let canonical = root.path().canonicalize().unwrap();
        let collected = collect_project_files(&canonical).unwrap();
        build_project_facts(&canonical, collected).unwrap()
    }

    #[test]
    fn lookup_finds_cross_file_usages_of_an_exported_function() {
        let project = project_from(&[
            ("lib.ts", "export function load() { return 1; }\n"),
            ("main.ts", "import { load } from './lib';\nload();\n"),
        ]);
        let report = lookup(&project, "load");
        assert_eq!(report.matches.len(), 1);
        assert_eq!(report.matches[0].declaration.file, "lib.ts");
        assert_eq!(report.matches[0].declaration.kind, "function");
        assert!(report.matches[0].declaration.exported);
        assert!(
            report.matches[0].usages.iter().any(|usage| usage.file == "main.ts"),
            "{:?}",
            report.matches[0].usages
        );
    }

    #[test]
    fn lookup_finds_same_file_usages_of_a_constant() {
        let project = project_from(&[(
            "util.ts",
            "export const MAX = 3;\nexport function cap(n: number) { return Math.min(n, MAX); }\n",
        )]);
        let report = lookup(&project, "MAX");
        assert_eq!(report.matches.len(), 1);
        assert_eq!(report.matches[0].declaration.kind, "variable");
        assert!(
            report.matches[0]
                .usages
                .iter()
                .any(|usage| usage.file == "util.ts"
                    && usage.line > report.matches[0].declaration.line),
            "{:?}",
            report.matches[0].usages
        );
    }
}
