mod worker;

pub use worker::{WorkerRequest, WorkerResponse, run_worker};

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::slop::types::ProjectFacts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTarget {
    pub file: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leftover {
    pub file: String,
    pub line: u32,
    pub text: String,
}

pub fn parse_target(raw: &str) -> RenameTarget {
    match raw.split_once('#') {
        Some((file, name)) => RenameTarget { file: Some(file.to_string()), name: name.to_string() },
        None => RenameTarget { file: None, name: raw.to_string() },
    }
}

pub fn resolve_declaration(project: &ProjectFacts, target: &RenameTarget) -> Result<(String, u32)> {
    let mut matches = Vec::new();
    for facts in project.files.values() {
        for declaration in &facts.declarations {
            if declaration.key.name != target.name {
                continue;
            }
            if let Some(file) = &target.file
                && declaration.span.path != *file
            {
                continue;
            }
            matches.push((declaration.span.path.clone(), declaration.span.start_line));
        }
    }
    matches.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    match matches.as_slice() {
        [] => bail!("no declaration named {}", target.name),
        [(file, line)] => Ok((file.clone(), *line)),
        // Same-file multi-matches are TS declaration merging (interface +
        // namespace + function/class/enum share one symbol); any site works.
        [(first_file, first_line), rest @ ..]
            if rest.iter().all(|(path, _)| path == first_file) =>
        {
            Ok((first_file.clone(), *first_line))
        }
        _ => {
            let listed = matches
                .iter()
                .map(|(path, _)| format!("{}#{}", path, target.name))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("ambiguous name {}; candidates: {listed}", target.name)
        }
    }
}

pub fn scan_leftovers(root: &Path, name: &str) -> Vec<Leftover> {
    let mut leftovers = Vec::new();
    let walker = WalkBuilder::new(root).standard_filters(true).hidden(false).build();
    for entry in walker.filter_map(Result::ok) {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let file = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        for (idx, line) in content.lines().enumerate() {
            if contains_identifier(line, name) {
                leftovers.push(Leftover {
                    file: file.clone(),
                    line: u32::try_from(idx + 1).unwrap_or(u32::MAX),
                    text: line.to_string(),
                });
            }
        }
    }
    leftovers.sort_by(|left, right| left.file.cmp(&right.file).then(left.line.cmp(&right.line)));
    leftovers
}

pub fn find_identifier_line(path: &Path, name: &str) -> Option<u32> {
    let content = fs::read_to_string(path).ok()?;
    for (idx, line) in content.lines().enumerate() {
        if contains_identifier(line, name) {
            return u32::try_from(idx + 1).ok();
        }
    }
    None
}

fn contains_identifier(line: &str, name: &str) -> bool {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let mut start = 0;
    while let Some(offset) = line[start..].find(name) {
        let hit = start + offset;
        let before_ok = line[..hit].chars().next_back().is_none_or(|c| !is_ident(c));
        let after_ok = line[hit + name.len()..].chars().next().is_none_or(|c| !is_ident(c));
        if before_ok && after_ok {
            return true;
        }
        start = hit + name.len();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::tempdir;

    use crate::slop::{build_project_facts, collect_project_files};

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
    fn parse_target_splits_file_qualifier() {
        let qualified = parse_target("src/card.tsx#Card");
        assert_eq!(qualified.file.as_deref(), Some("src/card.tsx"));
        assert_eq!(qualified.name, "Card");

        let bare = parse_target("Card");
        assert_eq!(bare.file, None);
        assert_eq!(bare.name, "Card");
    }

    #[test]
    fn resolve_declaration_returns_the_single_card() {
        let project = project_from(&[
            ("card.ts", "export function Card() { return 1; }\n"),
            ("app.ts", "import { Card } from './card';\nCard();\n"),
        ]);
        let (file, line) = resolve_declaration(&project, &parse_target("Card")).unwrap();
        assert_eq!(file, "card.ts");
        assert_eq!(line, 1);
    }

    #[test]
    fn resolve_declaration_merged_declarations_in_one_file_pick_first() {
        let project = project_from(&[(
            "card.ts",
            "export interface Card { id: string }\nexport namespace Card { export const kind = \"k\"; }\n",
        )]);
        let (file, line) = resolve_declaration(&project, &parse_target("Card")).unwrap();
        assert_eq!(file, "card.ts");
        assert_eq!(line, 1);
    }

    #[test]
    fn resolve_declaration_errors_when_two_cards_are_unqualified() {
        let project = project_from(&[
            ("a.ts", "export function Card() { return 1; }\n"),
            ("b.ts", "export const Card = 1;\n"),
        ]);
        let error = resolve_declaration(&project, &parse_target("Card")).unwrap_err().to_string();
        assert!(error.contains("a.ts#Card"), "{error}");
        assert!(error.contains("b.ts#Card"), "{error}");
    }

    #[test]
    fn contains_identifier_respects_word_boundaries() {
        assert!(contains_identifier("const label = \"Card\";", "Card"));
        assert!(contains_identifier("<Card />", "Card"));
        assert!(contains_identifier("Card", "Card"));
        assert!(!contains_identifier("<ProfileCard />", "Card"));
        assert!(!contains_identifier("Cardio()", "Card"));
        assert!(!contains_identifier("my_Card", "Card"));
        assert!(!contains_identifier("$Card", "Card"));
        assert!(contains_identifier("ProfileCard Card", "Card"));
    }

    #[test]
    fn scan_leftovers_finds_string_literal_and_skips_other_files() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("notes.ts"), "const label = \"Card\";\n").unwrap();
        fs::write(root.path().join("other.ts"), "const x = 1;\n").unwrap();
        fs::write(root.path().join("renamed.tsx"), "export const App = () => <ProfileCard />;\n")
            .unwrap();
        let leftovers = scan_leftovers(root.path(), "Card");
        assert_eq!(leftovers.len(), 1);
        assert_eq!(leftovers[0].file, "notes.ts");
        assert_eq!(leftovers[0].line, 1);
        assert!(leftovers[0].text.contains("Card"), "{}", leftovers[0].text);
    }
}
