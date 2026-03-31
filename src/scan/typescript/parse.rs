use std::path::Path;

use anyhow::{Context, Result};
use oxc::allocator::Allocator;
use oxc::parser::{ParseOptions, Parser};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;

use super::extract;
use crate::scan::types::{FileIndex, FunctionKindsFilter};

pub fn process_file(path: &Path, root: &Path, filter: FunctionKindsFilter) -> Result<FileIndex> {
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

    let parse_errors = parser_ret.errors.len();

    if parser_ret.panicked {
        tracing::warn!(path = %path.display(), "parser panicked");
        return Ok(FileIndex {
            path: rel_path,
            functions: Vec::new(),
            bindings: Vec::new(),
            exports: Vec::new(),

            parse_errors,
        });
    }

    let sem_ret = SemanticBuilder::new().build(&parser_ret.program);
    let semantic = sem_ret.semantic;

    let result = extract::extract_file(&parser_ret.program, &semantic, &source, filter);

    Ok(FileIndex {
        path: rel_path,
        functions: result.functions,
        bindings: result.bindings,
        exports: result.exports,
        parse_errors,
    })
}

#[cfg(test)]
#[path = "parse_test.rs"]
mod tests;
