use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::RenameArgs;
use crate::rename::{
    Leftover, WorkerRequest, WorkerResponse, find_identifier_line, parse_target,
    resolve_declaration, run_worker, scan_leftovers,
};
use crate::slop::{build_project_facts, collect_project_files};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RenameReport {
    ver: u8,
    renamed: RenamedSymbol,
    dry_run: bool,
    files: Vec<String>,
    leftovers: Vec<Leftover>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RenamedSymbol {
    name: String,
    new_name: String,
    file: String,
    line: u32,
}

pub fn run(args: &RenameArgs) -> Result<()> {
    let root = std::fs::canonicalize(&args.root).context("Cannot resolve project root")?;
    let tsconfig = args.tsconfig.as_ref().map_or_else(|| root.join("tsconfig.json"), PathBuf::from);
    anyhow::ensure!(tsconfig.exists(), "tsconfig not found at {}", tsconfig.display());
    let tsconfig = std::fs::canonicalize(&tsconfig).unwrap_or(tsconfig);
    let files = collect_project_files(&root)?;
    let project = build_project_facts(&root, files)?;
    let target = parse_target(&args.target);
    // Interface/type members are invisible to the facts layer; with an explicit
    // file qualifier the worker's type checker can still resolve them, so fall
    // back to locating the identifier in that file.
    let (file, line) = match resolve_declaration(&project, &target) {
        Ok(hit) => hit,
        Err(error) => match &target.file {
            Some(file) if error.to_string().starts_with("no declaration") => {
                let line = find_identifier_line(&root.join(file), &target.name)
                    .with_context(|| format!("no identifier {} in {file}", target.name))?;
                (file.clone(), line)
            }
            _ => return Err(error),
        },
    };
    let source_file = root.join(&file);
    let response: WorkerResponse = run_worker(&WorkerRequest {
        tsconfig: tsconfig.to_string_lossy().into_owned(),
        file: source_file.to_string_lossy().into_owned(),
        line,
        name: target.name.clone(),
        new_name: args.new_name.clone(),
        dry_run: args.dry_run,
    })?;
    let leftovers = if args.dry_run { Vec::new() } else { scan_leftovers(&root, &target.name) };
    let mut files: Vec<String> =
        response.files.iter().map(|path| root_relative(&root, path)).collect();
    files.sort();
    let report = RenameReport {
        ver: 1,
        renamed: RenamedSymbol { name: target.name, new_name: args.new_name.clone(), file, line },
        dry_run: args.dry_run,
        files,
        leftovers,
    };
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, &report)?;
    handle.write_all(b"\n")?;
    Ok(())
}

fn root_relative(root: &Path, path: &str) -> String {
    let path = Path::new(path);
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rename_report_round_trips_with_snake_case_fields() {
        let report = RenameReport {
            ver: 1,
            renamed: RenamedSymbol {
                name: "Card".to_string(),
                new_name: "ProfileCard".to_string(),
                file: "card.tsx".to_string(),
                line: 1,
            },
            dry_run: false,
            files: vec!["app.tsx".to_string(), "card.tsx".to_string()],
            leftovers: vec![Leftover {
                file: "notes.ts".to_string(),
                line: 1,
                text: "const label = \"Card\";".to_string(),
            }],
        };
        let value = serde_json::to_value(&report).unwrap();
        let object = value.as_object().unwrap();
        for key in ["ver", "renamed", "dry_run", "files", "leftovers"] {
            assert!(object.contains_key(key), "missing {key} in {object:?}");
        }
        let parsed: RenameReport = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, report);
    }

    #[test]
    fn missing_tsconfig_errors_before_facts_build() {
        let root = tempdir().unwrap();
        let args = RenameArgs {
            target: "Card".to_string(),
            new_name: "ProfileCard".to_string(),
            root: root.path().to_string_lossy().into_owned(),
            tsconfig: None,
            dry_run: true,
        };
        let error = run(&args).unwrap_err().to_string();
        assert!(error.contains("tsconfig"), "{error}");
        let expected = root.path().join("tsconfig.json");
        assert!(error.contains(&expected.display().to_string()), "{error}");
    }
}
