use super::*;
use crate::scan::types::{
    BindingInfo, BindingKind, ExportInfo, FileIndex, FunctionInfo, FunctionKind, ScanResult, Stats,
};

fn scan_result_example() -> ScanResult {
    let fi1 = FileIndex {
        path: "dir/a.ts".to_string(),
        functions: vec![
            FunctionInfo {
                name: Some("foo".into()),
                kind: FunctionKind::Declaration,
                exported: true,
                is_async: false,
                is_generator: false,
                line: 1,
                col: 1,
                line_end: 1,
            },
            FunctionInfo {
                name: None,
                kind: FunctionKind::Arrow,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 2,
                col: 1,
                line_end: 2,
            },
            FunctionInfo {
                name: Some("foo".into()),
                kind: FunctionKind::Declaration,
                exported: true,
                is_async: false,
                is_generator: false,
                line: 3,
                col: 1,
                line_end: 3,
            },
        ],
        bindings: vec![BindingInfo {
            name: "x".into(),
            kind: BindingKind::Const,
            exported: false,
            refs: 0,
            line: 1,
            col: 1,
        }],
        exports: vec![],
        parse_errors: 0,
    };
    let fi2 = FileIndex {
        path: "b.ts".to_string(),
        functions: vec![FunctionInfo {
            name: Some("bar".into()),
            kind: FunctionKind::Declaration,
            exported: false,
            is_async: false,
            is_generator: false,
            line: 1,
            col: 1,
            line_end: 1,
        }],
        bindings: vec![],
        exports: vec![],
        parse_errors: 0,
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
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 1,
                col: 1,
                line_end: 50,
            },
            FunctionInfo {
                name: Some("get".into()),
                kind: FunctionKind::ObjectMethod,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 10,
                col: 1,
                line_end: 20,
            },
        ],
        bindings: vec![],
        exports: vec![],
        parse_errors: 0,
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
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 1,
                col: 1,
                line_end: 100,
            },
            FunctionInfo {
                name: Some("get".into()),
                kind: FunctionKind::ObjectMethod,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 10,
                col: 1,
                line_end: 20,
            },
            FunctionInfo {
                name: Some("util".into()),
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 150,
                col: 1,
                line_end: 160,
            },
        ],
        bindings: vec![],
        exports: vec![],
        parse_errors: 0,
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
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 1,
                col: 1,
                line_end: 200,
            },
            FunctionInfo {
                name: Some("inner".into()),
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 20,
                col: 1,
                line_end: 150,
            },
            FunctionInfo {
                name: Some("leaf".into()),
                kind: FunctionKind::Declaration,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 50,
                col: 1,
                line_end: 60,
            },
        ],
        bindings: vec![],
        exports: vec![],
        parse_errors: 0,
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
    assert!(names.contains(&"inner.leaf".to_string()));
    assert!(!names.contains(&"outer.leaf".to_string()));
}

#[test]
fn write_result_emits_valid_json_all_modes() {
    let fi = FileIndex {
        path: "p.ts".into(),
        functions: vec![
            FunctionInfo {
                name: Some("parent".into()),
                kind: FunctionKind::Declaration,
                exported: true,
                is_async: false,
                is_generator: false,
                line: 1,
                col: 1,
                line_end: 50,
            },
            FunctionInfo {
                name: Some("child".into()),
                kind: FunctionKind::ObjectMethod,
                exported: false,
                is_async: false,
                is_generator: false,
                line: 10,
                col: 1,
                line_end: 20,
            },
        ],
        bindings: vec![BindingInfo {
            name: "x".into(),
            kind: BindingKind::Const,
            exported: false,
            refs: 0,
            line: 1,
            col: 1,
        }],
        exports: vec![],
        parse_errors: 0,
    };
    let r = ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 1, parsed: 1, skipped: 0, errors: 0 },
        file_indices: vec![fi],
        errors: vec![],
    };
    for mode in [OutputMode::Compact, OutputMode::Verbose, OutputMode::Files, OutputMode::Folders] {
        let mut buf = Vec::new();
        write_result(&r, mode, &mut buf).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert!(v.get("ver").is_some());
    }
}

#[test]
fn verbose_output_includes_exports_and_folder_none_parent_path() {
    let fi = FileIndex {
        path: String::new(),
        functions: vec![FunctionInfo {
            name: Some("top".into()),
            kind: FunctionKind::Declaration,
            exported: true,
            is_async: false,
            is_generator: false,
            line: 1,
            col: 1,
            line_end: 1,
        }],
        bindings: vec![],
        exports: vec![ExportInfo { name: "default".into(), kind_code: 2 }],
        parse_errors: 0,
    };
    let r = ScanResult {
        ver: 1,
        root: ".".into(),
        stats: Stats { files: 1, parsed: 1, skipped: 0, errors: 0 },
        file_indices: vec![fi],
        errors: vec![],
    };

    let mut verbose = Vec::new();
    write_result(&r, OutputMode::Verbose, &mut verbose).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&verbose).unwrap();
    assert_eq!(v["exports"][0]["name"], "default");
    assert_eq!(v["exports"][0]["kindCode"], 2);

    let folders = FoldersOutput::from(&r);
    assert!(folders.folders.contains_key("."));
}
