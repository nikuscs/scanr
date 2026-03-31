use super::*;
use crate::scan::types::FunctionKindsFilter;
use std::fs;
use tempfile::TempDir;

#[test]
fn parses_basic_ts_and_sets_relative_path() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let file = root.join("a.ts");
    fs::write(
        &file,
        r"
            export function foo() {}
            export const bar = () => {};
            class C { get x(){ return 1 } }
        ",
    )
    .unwrap();

    let root_canon = root.canonicalize().unwrap();
    let fi = process_file(&file, &root_canon, FunctionKindsFilter::All).unwrap();
    assert_eq!(fi.path, "a.ts");
    let names: Vec<_> = fi.functions.iter().filter_map(|f| f.name.clone()).collect();
    assert!(names.contains(&"foo".to_string()));
    assert!(names.contains(&"bar".to_string()));
    assert_eq!(fi.parse_errors, 0);
}

#[test]
fn unsupported_extension_errors() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let file = root.join("a.unknownext");
    fs::write(&file, "let x = 1;\n").unwrap();
    let root_canon = root.canonicalize().unwrap();
    let err = process_file(&file, &root_canon, FunctionKindsFilter::All).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("unsupported file type"));
}

#[test]
fn parse_records_syntax_errors() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let file = root.join("bad.ts");
    fs::write(&file, "export function ( {\n").unwrap();
    let root_canon = root.canonicalize().unwrap();
    let fi = process_file(&file, &root_canon, FunctionKindsFilter::All).unwrap();
    assert_eq!(fi.path, "bad.ts");
    assert!(fi.parse_errors > 0);
    assert!(fi.functions.is_empty());
}

#[test]
fn missing_file_returns_error() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let file = root.join("missing.ts");
    let root_canon = root.canonicalize().unwrap();
    let err = process_file(&file, &root_canon, FunctionKindsFilter::All).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("failed to read"));
}

#[test]
fn uses_canonical_path_when_root_mismatch() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();
    let file = dir1.path().join("a.ts");
    std::fs::write(&file, "export function f(){}\n").unwrap();
    let root_canon = dir2.path().canonicalize().unwrap();
    let fi = process_file(&file, &root_canon, FunctionKindsFilter::All).unwrap();
    assert!(std::path::Path::new(&fi.path).is_absolute());
    assert!(fi.path.ends_with("a.ts"));
}

#[test]
fn parses_js_file() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let file = root.join("b.js");
    fs::write(&file, "function g(){}\n").unwrap();
    let root_canon = root.canonicalize().unwrap();
    let fi = process_file(&file, &root_canon, FunctionKindsFilter::All).unwrap();
    assert!(fi.functions.iter().filter_map(|f| f.name.as_ref()).any(|n| n == "g"));
}
