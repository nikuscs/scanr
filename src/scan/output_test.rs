use super::*;
use crate::scan::types::{
    BindingInfo, BindingKind, ClassInfo, ExportInfo, FileIndex, FunctionContent, FunctionInfo,
    FunctionKind, ScanResult, Stats, TypeDeclKind, TypeInfo, Violation,
};
use crate::slop::types::FileFacts;

fn scan_result_example() -> ScanResult {
    let fi1 = FileIndex {
        path: "dir/a.ts".to_string(),
        functions: vec![
            FunctionInfo {
                name: Some("foo".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: true,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 1,
                col: 1,
                line_end: 1,
            },
            FunctionInfo {
                name: None,
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Arrow,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 2,
                col: 1,
                line_end: 2,
            },
            FunctionInfo {
                name: Some("foo".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: true,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 3,
                col: 1,
                line_end: 3,
            },
        ],
        classes: vec![],
        bindings: vec![BindingInfo {
            name: "x".into(),
            kind: BindingKind::Const,
            exported: false,
            refs: 0,
            line: 1,
            col: 1,
        }],
        types: vec![],
        exports: vec![],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
    };
    let fi2 = FileIndex {
        path: "b.ts".to_string(),
        functions: vec![FunctionInfo {
            name: Some("bar".into()),
            parent: None,
            captures: Vec::new(),
            low_value_reason: None,
            kind: FunctionKind::Declaration,
            exported: false,
            is_async: false,
            is_generator: false,
            content: FunctionContent::Plain,
            line: 1,
            col: 1,
            line_end: 1,
        }],
        classes: vec![],
        bindings: vec![],
        types: vec![],
        exports: vec![],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
    };
    ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 2, parsed: 2, skipped: 0, errors: 0 },
        file_indices: vec![fi1, fi2],
        errors: vec![],
    }
}

#[test]
fn files_mode_groups_named_functions() {
    let r = scan_result_example();
    let files = FilesOutput::from(&r);

    assert_eq!(files.ver, 1);
    assert_eq!(files.stats.parsed, 2);
    assert_eq!(files.files.get("dir/a.ts").unwrap(), &vec!["foo".to_string()]);
    assert_eq!(files.files.get("b.ts").unwrap(), &vec!["bar".to_string()]);
}

#[test]
fn folders_mode_summarizes_by_parent_dir() {
    let r = scan_result_example();
    let folders = FoldersOutput::from(&r);

    let dir = folders.folders.get("dir").unwrap();
    assert_eq!(dir.functions, 3);
    assert_eq!(dir.names, vec!["foo".to_string()]);

    let root_dir = folders.folders.get(".").unwrap();
    assert_eq!(root_dir.functions, 1);
    assert_eq!(root_dir.names, vec!["bar".to_string()]);
}

#[test]
fn folders_mode_uses_dot_names() {
    let fi = FileIndex {
        path: "dir/x.ts".into(),
        functions: vec![
            FunctionInfo {
                name: Some("builder".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 1,
                col: 1,
                line_end: 50,
            },
            FunctionInfo {
                name: Some("get".into()),
                parent: Some("builder".into()),
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::ObjectMethod,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 10,
                col: 1,
                line_end: 20,
            },
        ],
        classes: vec![],
        bindings: vec![],
        types: vec![],
        exports: vec![],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
    };
    let r = ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 1, parsed: 1, skipped: 0, errors: 0 },
        file_indices: vec![fi],
        errors: vec![],
    };
    let folders = FoldersOutput::from(&r);
    let entry = folders.folders.get("dir").unwrap();
    assert!(entry.names.contains(&"builder.get".to_string()));
    assert!(entry.names.contains(&"builder".to_string()));
}

#[test]
fn dot_names_for_nested_methods() {
    let fi = FileIndex {
        path: "x.ts".into(),
        functions: vec![
            FunctionInfo {
                name: Some("builder".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 1,
                col: 1,
                line_end: 100,
            },
            FunctionInfo {
                name: Some("get".into()),
                parent: Some("builder".into()),
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::ObjectMethod,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 10,
                col: 1,
                line_end: 20,
            },
            FunctionInfo {
                name: Some("util".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 150,
                col: 1,
                line_end: 160,
            },
        ],
        classes: vec![],
        bindings: vec![],
        types: vec![],
        exports: vec![],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
    };
    let r = ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 1, parsed: 1, skipped: 0, errors: 0 },
        file_indices: vec![fi],
        errors: vec![],
    };
    let files = FilesOutput::from(&r);
    let names = files.files.get("x.ts").unwrap();
    assert!(names.contains(&"builder.get".to_string()));
    assert!(names.contains(&"builder".to_string()));
    assert!(names.contains(&"util".to_string()));
}

#[test]
fn dot_names_picks_nearest_parent() {
    let fi = FileIndex {
        path: "x.ts".into(),
        functions: vec![
            FunctionInfo {
                name: Some("outer".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 1,
                col: 1,
                line_end: 200,
            },
            FunctionInfo {
                name: Some("inner".into()),
                parent: Some("outer".into()),
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 20,
                col: 1,
                line_end: 150,
            },
            FunctionInfo {
                name: Some("leaf".into()),
                parent: Some("outer.inner".into()),
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 50,
                col: 1,
                line_end: 60,
            },
        ],
        classes: vec![],
        bindings: vec![],
        types: vec![],
        exports: vec![],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
    };
    let names = FilesOutput::from(&ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 1, parsed: 1, skipped: 0, errors: 0 },
        file_indices: vec![fi],
        errors: vec![],
    })
    .files
    .remove("x.ts")
    .unwrap();
    assert!(names.contains(&"outer.inner.leaf".to_string()));
    assert!(!names.contains(&"outer.leaf".to_string()));
}

#[test]
fn write_result_emits_valid_json_all_modes() {
    let fi = FileIndex {
        path: "p.ts".into(),
        functions: vec![
            FunctionInfo {
                name: Some("parent".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: true,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 1,
                col: 1,
                line_end: 50,
            },
            FunctionInfo {
                name: Some("child".into()),
                parent: Some("parent".into()),
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::ObjectMethod,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 10,
                col: 1,
                line_end: 20,
            },
        ],
        classes: vec![],
        bindings: vec![BindingInfo {
            name: "x".into(),
            kind: BindingKind::Const,
            exported: false,
            refs: 0,
            line: 1,
            col: 1,
        }],
        types: vec![],
        exports: vec![],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
    };
    let r = ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 1, parsed: 1, skipped: 0, errors: 0 },
        file_indices: vec![fi],
        errors: vec![],
    };
    for mode in [
        OutputMode::Compact,
        OutputMode::Verbose,
        OutputMode::Files,
        OutputMode::Folders,
        OutputMode::Inventory,
    ] {
        let mut buf = Vec::new();
        write_result_with_lines(&r, mode, false, &mut buf).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert!(v.get("ver").is_some());
    }
}

#[test]
fn compact_and_verbose_outputs_include_violations() {
    let mut result = scan_result_example();
    result.file_indices[0].violations.push(Violation {
        rule: "demo".into(),
        count: 1,
        details: vec!["foo".into()],
    });

    let mut compact = Vec::new();
    write_result_with_lines(&result, OutputMode::Compact, false, &mut compact).unwrap();
    let compact: serde_json::Value = serde_json::from_slice(&compact).unwrap();
    assert_eq!(compact["viol"][0][1], "demo");

    let mut verbose = Vec::new();
    write_result_with_lines(&result, OutputMode::Verbose, false, &mut verbose).unwrap();
    let verbose: serde_json::Value = serde_json::from_slice(&verbose).unwrap();
    assert_eq!(verbose["violations"][0]["rule"], "demo");
}

#[test]
fn verbose_output_includes_exports_and_folder_none_parent_path() {
    let fi = FileIndex {
        path: String::new(),
        functions: vec![FunctionInfo {
            name: Some("top".into()),
            parent: None,
            captures: Vec::new(),
            low_value_reason: None,
            kind: FunctionKind::Declaration,
            exported: true,
            is_async: false,
            is_generator: false,
            content: FunctionContent::Plain,
            line: 1,
            col: 1,
            line_end: 1,
        }],
        classes: vec![],
        bindings: vec![],
        types: vec![],
        exports: vec![ExportInfo { name: "default".into(), kind_code: 2 }],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
    };
    let r = ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 1, parsed: 1, skipped: 0, errors: 0 },
        file_indices: vec![fi],
        errors: vec![],
    };

    let mut verbose = Vec::new();
    write_result_with_lines(&r, OutputMode::Verbose, false, &mut verbose).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&verbose).unwrap();
    assert_eq!(v["exports"][0]["name"], "default");
    assert_eq!(v["exports"][0]["kindCode"], 2);

    let folders = FoldersOutput::from(&r);
    assert!(folders.folders.contains_key("."));
}

#[test]
fn compact_and_verbose_outputs_include_types() {
    let mut result = scan_result_example();
    result.file_indices[0].types.push(TypeInfo {
        name: "Range".into(),
        kind: TypeDeclKind::Interface,
        exported: true,
        line: 4,
        col: 8,
    });
    result.file_indices[0].types.push(TypeInfo {
        name: "Local".into(),
        kind: TypeDeclKind::TypeAlias,
        exported: false,
        line: 9,
        col: 6,
    });

    let mut compact = Vec::new();
    write_result_with_lines(&result, OutputMode::Compact, false, &mut compact).unwrap();
    let compact: serde_json::Value = serde_json::from_slice(&compact).unwrap();
    assert_eq!(compact["t"][0], serde_json::json!(["dir/a.ts", 4, 8, "Range", 1, "interface"]));
    assert_eq!(compact["t"][1], serde_json::json!(["dir/a.ts", 9, 6, "Local", 0, "type"]));

    let mut verbose = Vec::new();
    write_result_with_lines(&result, OutputMode::Verbose, false, &mut verbose).unwrap();
    let verbose: serde_json::Value = serde_json::from_slice(&verbose).unwrap();
    assert_eq!(verbose["types"][0]["name"], "Range");
    assert_eq!(verbose["types"][0]["kind"], "interface");
    assert_eq!(verbose["types"][0]["exported"], true);
    assert_eq!(verbose["types"][1]["name"], "Local");
    assert_eq!(verbose["types"][1]["kind"], "type");
    assert_eq!(verbose["types"][1]["exported"], false);
}

#[test]
fn compact_schema_describes_positional_arrays() {
    let mut buf = Vec::new();
    write_schema(OutputMode::Compact, &mut buf).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    assert_eq!(schema["mode"], "compact");
    assert_eq!(schema["ver"], 1);
    assert_eq!(
        schema["fields"]["t"],
        serde_json::json!(["file", "line", "col", "name", "exported", "kind"])
    );
    assert_eq!(schema["fields"]["f"][4], "exported");
    assert_eq!(schema["fields"]["x"], serde_json::json!(["file", "name", "kind_code"]));
    assert_eq!(schema["export_kind_codes"]["1"], "named");
}

fn inventory_scan_result() -> ScanResult {
    let fi = FileIndex {
        path: "Card.tsx".into(),
        functions: vec![
            FunctionInfo {
                name: Some("Card".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Arrow,
                exported: true,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Jsx,
                line: 10,
                col: 1,
                line_end: 20,
            },
            FunctionInfo {
                name: Some("useCard".into()),
                parent: None,
                captures: Vec::new(),
                low_value_reason: None,
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                content: FunctionContent::Plain,
                line: 22,
                col: 1,
                line_end: 24,
            },
        ],
        classes: vec![ClassInfo { name: "Svc".into(), exported: true, line: 40, line_end: 50 }],
        bindings: vec![
            BindingInfo {
                name: "MAX".into(),
                kind: BindingKind::Const,
                exported: true,
                refs: 2,
                line: 3,
                col: 1,
            },
            BindingInfo {
                name: "Card".into(),
                kind: BindingKind::Const,
                exported: true,
                refs: 1,
                line: 10,
                col: 1,
            },
            BindingInfo {
                name: "Kind".into(),
                kind: BindingKind::Enum,
                exported: true,
                refs: 1,
                line: 30,
                col: 1,
            },
        ],
        types: vec![TypeInfo {
            name: "Props".into(),
            kind: TypeDeclKind::Interface,
            exported: true,
            line: 1,
            col: 1,
        }],
        exports: vec![ExportInfo { name: "Card".into(), kind_code: 1 }],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
    };
    ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 1, parsed: 1, skipped: 0, errors: 0 },
        file_indices: vec![fi],
        errors: vec![],
    }
}

#[test]
fn inventory_lists_names_without_lines_by_default() {
    let mut buf = Vec::new();
    write_result_with_lines(&inventory_scan_result(), OutputMode::Inventory, false, &mut buf)
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    assert_eq!(
        v["functions"],
        serde_json::json!([{"file": "Card.tsx", "name": "Card"}, {"file": "Card.tsx", "name": "useCard"}])
    );
    assert_eq!(
        v["constants"],
        serde_json::json!([
            {"file": "Card.tsx", "name": "Card", "kind": "arrow"},
            {"file": "Card.tsx", "name": "MAX", "kind": "primitive"}
        ])
    );
    assert_eq!(
        v["types"],
        serde_json::json!([{"file": "Card.tsx", "name": "Props", "kind": "interface"}])
    );
    assert_eq!(v["components"], serde_json::json!([{"file": "Card.tsx", "name": "Card"}]));
    assert_eq!(v["hooks"], serde_json::json!([{"file": "Card.tsx", "name": "useCard"}]));
    assert_eq!(v["classes"], serde_json::json!([{"file": "Card.tsx", "name": "Svc"}]));
    assert_eq!(v["enums"], serde_json::json!([{"file": "Card.tsx", "name": "Kind"}]));
    assert_eq!(v["exports"], serde_json::json!([{"file": "Card.tsx", "name": "Card"}]));
    assert!(v["functions"][0].get("line").is_none());
}

#[test]
fn inventory_includes_lines_when_requested() {
    let mut buf = Vec::new();
    write_result_with_lines(&inventory_scan_result(), OutputMode::Inventory, true, &mut buf)
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    assert_eq!(v["constants"][1]["name"], "MAX");
    assert_eq!(v["constants"][1]["line"], 3);
    assert_eq!(v["components"][0]["line"], 10);
}
