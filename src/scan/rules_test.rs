use super::*;
use crate::scan::types::{BindingInfo, FileIndex, FunctionInfo, FunctionKind};

fn mk_fi(path: &str, fn_names: &[&str], binding_unused: bool) -> FileIndex {
    let functions = fn_names
        .iter()
        .map(|n| FunctionInfo {
            name: Some((*n).into()),
            kind: FunctionKind::Declaration,
            exported: true,
            is_async: false,
            is_generator: false,
            line: 1,
            col: 1,
            line_end: 1,
        })
        .collect();
    let bindings = if binding_unused {
        vec![BindingInfo {
            name: "tmp".into(),
            kind: BindingKind::Const,
            exported: false,
            refs: 0,
            line: 1,
            col: 1,
        }]
    } else {
        vec![]
    };
    FileIndex {
        path: path.into(),
        functions,
        bindings,
        exports: vec![],
        violations: vec![],
        parse_errors: 0,
    }
}

#[test]
fn no_unused_bindings_flags_zero_ref_non_imports() {
    let mut fi = mk_fi("a.ts", &[], true);
    run_rules(&[], &mut fi);
    assert!(fi.violations.iter().any(|v| v.rule == "no_unused_bindings" && v.count == 1));
}

#[test]
fn one_exported_function_per_file_flags_multiple() {
    let mut fi = mk_fi("src/a.ts", &["a", "b"], false);
    run_rules(&["one_exported_function_per_file".into()], &mut fi);
    assert!(
        fi.violations.iter().any(|v| v.rule == "one_exported_function_per_file" && v.count == 2)
    );
}

#[test]
fn max_functions_per_file_flags_excess() {
    let fi = mk_fi("a.ts", &["a", "b", "c"], false);
    let v = MaxFunctionsPerFile { max: 2 }.check(&fi).unwrap();
    assert_eq!(v.rule, "max_functions_per_file");
    assert_eq!(v.count, 3);
}

#[test]
fn run_rules_filters_by_enabled_names() {
    let mut fi = mk_fi("a.ts", &[], true);
    run_rules(&["max_functions_per_file".into()], &mut fi);
    assert!(fi.violations.is_empty());
}

#[test]
fn no_unused_bindings_ignores_imports_and_prefixed_names() {
    let mut fi = mk_fi("a.ts", &[], false);
    fi.bindings = vec![
        BindingInfo {
            name: "_tmp".into(),
            kind: BindingKind::Const,
            exported: false,
            refs: 0,
            line: 1,
            col: 1,
        },
        BindingInfo {
            name: "imp".into(),
            kind: BindingKind::Import,
            exported: false,
            refs: 0,
            line: 1,
            col: 1,
        },
    ];
    run_rules(&["no_unused_bindings".into()], &mut fi);
    assert!(fi.violations.is_empty());
}

#[test]
fn one_exported_function_path_prefix_can_skip_file() {
    let fi = mk_fi("src/a.ts", &["a", "b"], false);
    let rule = OneExportedFunctionPerFile { path_prefix: Some("other/".into()) };
    assert!(rule.check(&fi).is_none());
}
