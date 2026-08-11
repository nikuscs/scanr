#![allow(dead_code)]
#![allow(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

// Vendored from github.com/mizchi/similarity, similarity-core 0.5.2 (MIT).
// Copyright (c) 2024 mizchi. See LICENSE in this directory.

pub mod apted;
pub mod ast_exchange;
pub mod ast_fingerprint;
pub mod class_extractor;
pub mod fast_similarity;
pub mod function_extractor;
mod ignore_directive;
pub mod parser;
pub mod structure_comparator;
pub mod subtree_fingerprint;
pub mod tree;
pub mod tsed;
pub mod type_comparator;
pub mod type_extractor;
pub mod type_fingerprint;
pub mod type_normalizer;
pub mod typescript_structure_adapter;
pub mod unified_type_comparator;

#[cfg(test)]
#[path = "tests/function_similarity_test.rs"]
mod function_similarity_test;
