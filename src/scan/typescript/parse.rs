use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use oxc::allocator::Allocator;
use oxc::parser::{ParseOptions, Parser};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;

use super::extract;
use crate::scan::health::{HealthAstMetrics, analyze_program};
use crate::scan::types::{FileIndex, FunctionKindsFilter};
use crate::similarity::function_extractor::{FunctionDefinition, extract_functions_from_program};
use crate::similarity::type_extractor::{TypeDefinition, TypeExtractor, TypeLiteralDefinition};
use crate::slop::facts::collect_facts;
use crate::slop::types::FileFacts;

#[derive(Clone)]
pub struct SimilarityFile {
    pub path: String,
    pub source: String,
    pub functions: Vec<FunctionDefinition>,
    pub types: Vec<TypeDefinition>,
    pub type_literals: Vec<TypeLiteralDefinition>,
    pub health: HealthAstMetrics,
    pub slop: FileFacts,
}

pub fn process_file(path: &Path, root: &Path, filter: FunctionKindsFilter) -> Result<FileIndex> {
    process_file_inner(path, root, filter, false).map(|(index, _)| index)
}

pub fn process_file_with_similarity(
    path: &Path,
    root: &Path,
    filter: FunctionKindsFilter,
) -> Result<(FileIndex, SimilarityFile)> {
    let (index, similarity) = process_file_inner(path, root, filter, true)?;
    let similarity = similarity.context("similarity extraction was not produced")?;
    Ok((index, similarity))
}

fn process_file_inner(
    path: &Path,
    root: &Path,
    filter: FunctionKindsFilter,
    include_similarity: bool,
) -> Result<(FileIndex, Option<SimilarityFile>)> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let source_type = SourceType::from_path(path)
        .map_err(|_| anyhow::anyhow!("unsupported file type: {}", path.display()))?;

    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let rel_path =
        canonical_path.strip_prefix(root).unwrap_or(&canonical_path).to_string_lossy().to_string();

    let allocator = Allocator::default();
    let parser_ret =
        Parser::new(&allocator, &source, source_type).with_options(ParseOptions::default()).parse();
    let parse_errors = parser_ret.diagnostics.len();

    if parser_ret.panicked {
        let index = FileIndex {
            path: rel_path.clone(),
            functions: Vec::new(),
            classes: Vec::new(),
            bindings: Vec::new(),
            exports: Vec::new(),
            violations: Vec::new(),
            parse_errors,
            slop: FileFacts {
                path: rel_path.clone(),
                analysis_complete: false,
                ..FileFacts::default()
            },
        };
        let similarity = include_similarity.then(|| SimilarityFile {
            path: rel_path,
            source,
            functions: Vec::new(),
            types: Vec::new(),
            type_literals: Vec::new(),
            health: HealthAstMetrics::default(),
            slop: index.slop.clone(),
        });
        return Ok((index, similarity));
    }

    let semantic =
        SemanticBuilder::new().with_build_nodes(true).build(&parser_ret.program).semantic;
    let result = extract::extract_file(&parser_ret.program, &semantic, &source, filter);
    let exported_names =
        result.exports.iter().map(|export| export.name.clone()).collect::<BTreeSet<_>>();
    let slop = collect_facts(
        &parser_ret.program,
        &semantic,
        &source,
        &rel_path,
        &result.functions,
        &exported_names,
        parse_errors == 0,
    );
    let similarity = include_similarity.then(|| {
        let type_extractor = TypeExtractor::new(source.clone(), rel_path.clone());
        SimilarityFile {
            path: rel_path.clone(),
            functions: extract_functions_from_program(&source, &parser_ret.program),
            types: type_extractor.extract_types_from_program(&parser_ret.program),
            type_literals: type_extractor.extract_type_literals_from_program(&parser_ret.program),
            health: analyze_program(&parser_ret.program),
            slop: slop.clone(),
            source: source.clone(),
        }
    });

    let index = FileIndex {
        path: rel_path,
        functions: result.functions,
        classes: result.classes,
        bindings: result.bindings,
        exports: result.exports,
        violations: Vec::new(),
        parse_errors,
        slop,
    };
    Ok((index, similarity))
}

#[cfg(test)]
#[path = "parse_test.rs"]
mod tests;
