use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use serde::Serialize;

use crate::cli::DupesArgs;
use crate::scan::types::FunctionKindsFilter;
use crate::scan::typescript::parse::{SimilarityFile, process_file_with_similarity};
use crate::scan::{ScanConfig, collect_files};
use crate::similarity::ast_fingerprint::AstFingerprint;
use crate::similarity::function_extractor::{
    FunctionDefinition, SimilarityResult, compare_functions,
};
use crate::similarity::tsed::TSEDOptions;
use crate::similarity::type_comparator::TypeComparisonOptions;
use crate::similarity::unified_type_comparator::{UnifiedType, find_similar_unified_types};

#[derive(Serialize)]
struct DupesOutput {
    functions: Vec<FunctionPair>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    types: Vec<TypePair>,
}

#[derive(Serialize)]
struct FunctionPair {
    similarity: f64,
    impact: u32,
    first: CodeLocation,
    second: CodeLocation,
}

#[derive(Serialize)]
struct TypePair {
    similarity: f64,
    first: CodeLocation,
    second: CodeLocation,
}

#[derive(Serialize)]
struct CodeLocation {
    path: String,
    line: usize,
    end_line: usize,
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

struct FunctionEntry {
    file_index: usize,
    function: FunctionDefinition,
    fingerprint: AstFingerprint,
}

pub fn run(args: &DupesArgs) -> Result<()> {
    let output = analyze(args)?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, &output)?;
    writeln!(out)?;
    Ok(())
}

fn analyze(args: &DupesArgs) -> Result<DupesOutput> {
    if !(0.0..=1.0).contains(&args.threshold) {
        bail!("threshold must be between 0 and 1");
    }
    if args.min_lines == 0 {
        bail!("min-lines must be at least 1");
    }

    let root = fs::canonicalize(&args.root).context("Cannot resolve project root")?;
    let config = ScanConfig::default();
    let files = collect_files(&root, &config)?;
    let parsed: Vec<Result<SimilarityFile>> = files
        .par_iter()
        .map(|path| {
            process_file_with_similarity(path, &root, FunctionKindsFilter::All)
                .map(|(_, similarity)| similarity)
        })
        .collect();
    let parsed: Vec<SimilarityFile> = parsed.into_iter().collect::<Result<_>>()?;

    let functions = compare_function_pairs(&parsed, args)?;
    let types = if args.types { compare_type_pairs(&parsed, args) } else { Vec::new() };
    Ok(DupesOutput { functions, types })
}

fn compare_function_pairs(files: &[SimilarityFile], args: &DupesArgs) -> Result<Vec<FunctionPair>> {
    let entries: Vec<FunctionEntry> = files
        .iter()
        .enumerate()
        .flat_map(|(file_index, file)| {
            file.functions
                .iter()
                .filter(|function| {
                    !function.has_ignore_directive && function.line_count() >= args.min_lines
                })
                .filter_map(move |function| {
                    let start = function.body_span.start as usize;
                    let end = function.body_span.end as usize;
                    let body = file.source.get(start..end)?;
                    let fingerprint = AstFingerprint::from_source(body).ok()?;
                    Some(FunctionEntry { file_index, function: function.clone(), fingerprint })
                })
        })
        .collect();
    let options = TSEDOptions { min_lines: args.min_lines, ..TSEDOptions::default() };

    let rows: Vec<Result<Vec<FunctionPair>>> = (0..entries.len())
        .into_par_iter()
        .map(|left_index| {
            let left = &entries[left_index];
            let left_file = &files[left.file_index];
            let mut matches = Vec::new();
            for right in &entries[(left_index + 1)..] {
                if !left.fingerprint.might_be_similar(&right.fingerprint, 0.5)
                    || left.fingerprint.similarity(&right.fingerprint) < 0.5
                {
                    continue;
                }
                let right_file = &files[right.file_index];
                let similarity = compare_functions(
                    &left.function,
                    &right.function,
                    &left_file.source,
                    &right_file.source,
                    &options,
                )
                .map_err(anyhow::Error::msg)?;
                if similarity < args.threshold {
                    continue;
                }

                let result = SimilarityResult::new(
                    left.function.clone(),
                    right.function.clone(),
                    similarity,
                );
                matches.push(FunctionPair {
                    similarity: result.similarity,
                    impact: result.impact,
                    first: function_location(left_file, &result.func1, args.print),
                    second: function_location(right_file, &result.func2, args.print),
                });
            }
            Ok(matches)
        })
        .collect();

    let mut pairs: Vec<_> =
        rows.into_iter().collect::<Result<Vec<_>>>()?.into_iter().flatten().collect();
    pairs.sort_by(|left, right| {
        location_key(&left.first, &left.second).cmp(&location_key(&right.first, &right.second))
    });
    Ok(pairs)
}

fn compare_type_pairs(files: &[SimilarityFile], args: &DupesArgs) -> Vec<TypePair> {
    let definitions: Vec<_> = files
        .iter()
        .flat_map(|file| file.types.iter())
        .filter(|type_def| !type_def.has_ignore_directive)
        .cloned()
        .collect();
    let literals: Vec<_> =
        files.iter().flat_map(|file| file.type_literals.iter()).cloned().collect();
    let options = TypeComparisonOptions::default();
    let sources: BTreeMap<_, _> =
        files.iter().map(|file| (file.path.as_str(), file.source.as_str())).collect();

    let mut pairs: Vec<_> =
        find_similar_unified_types(&definitions, &literals, args.threshold, &options)
            .into_iter()
            .map(|pair| TypePair {
                similarity: pair.result.similarity,
                first: type_location(&pair.type1, &sources, args.print),
                second: type_location(&pair.type2, &sources, args.print),
            })
            .collect();
    pairs.sort_by(|left, right| {
        location_key(&left.first, &left.second).cmp(&location_key(&right.first, &right.second))
    });
    pairs
}

fn function_location(
    file: &SimilarityFile,
    function: &FunctionDefinition,
    print: bool,
) -> CodeLocation {
    let source = print.then(|| {
        let start = function.body_span.start as usize;
        let end = function.body_span.end as usize;
        file.source.get(start..end).unwrap_or_default().to_string()
    });
    CodeLocation {
        path: file.path.clone(),
        line: function.start_line as usize,
        end_line: function.end_line as usize,
        name: function.name.clone(),
        kind: format!("{:?}", function.function_type).to_ascii_lowercase(),
        source,
    }
}

fn type_location(
    unified: &UnifiedType,
    sources: &BTreeMap<&str, &str>,
    print: bool,
) -> CodeLocation {
    let source = print.then(|| {
        sources.get(unified.file_path()).map_or_else(String::new, |source| {
            extract_lines(source, unified.start_line(), unified.end_line())
        })
    });
    CodeLocation {
        path: unified.file_path().to_string(),
        line: unified.start_line(),
        end_line: unified.end_line(),
        name: unified.name().to_string(),
        kind: unified.type_string(),
        source,
    }
}

fn extract_lines(source: &str, start_line: usize, end_line: usize) -> String {
    source
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(end_line.saturating_sub(start_line) + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

fn location_key<'a>(
    first: &'a CodeLocation,
    second: &'a CodeLocation,
) -> (&'a str, usize, &'a str, &'a str, usize, &'a str) {
    (&first.path, first.line, &first.name, &second.path, second.line, &second.name)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn analyze_finds_function_and_type_pairs() {
        let root = tempdir().unwrap();
        fs::write(
            root.path().join("first.ts"),
            "export interface DialogProps { value: string; enabled: boolean }\n\
             export function calculate(values: number[]) {\n\
               let total = 0;\n\
               for (const value of values) { total += value; }\n\
               return total;\n\
             }\n",
        )
        .unwrap();
        fs::write(
            root.path().join("second.ts"),
            "export interface DialogProps { value: string; enabled: boolean }\n\
             export function compute(items: number[]) {\n\
               let sum = 0;\n\
               for (const item of items) { sum += item; }\n\
               return sum;\n\
             }\n",
        )
        .unwrap();

        let args = DupesArgs {
            root: root.path().to_string_lossy().into_owned(),
            threshold: 0.0,
            min_lines: 1,
            types: true,
            print: true,
        };
        let output = analyze(&args).unwrap();
        assert_eq!(output.functions.len(), 1);
        assert!(output.functions[0].similarity > 0.0);
        assert_eq!(output.types.len(), 1);
        assert!(output.functions[0].first.source.is_some());
    }
}
