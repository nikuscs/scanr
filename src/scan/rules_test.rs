use super::*;
use crate::scan::types::{BindingInfo, FileIndex, FunctionInfo, FunctionKind};

fn mk_fi(path: &str, fn_names: &[&str], binding_unused: bool) -> FileIndex {
    let functions = fn_names
        .iter()
        .map(|n| FunctionInfo {
            name: Some((*n).into()),
            parent: None,
            captures: Vec::new(),
            low_value_reason: None,
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
fn hoistable_nested_function_requires_no_captures() {
    let mut fi = mk_fi("src/component.tsx", &["Component", "hoistable", "captured"], false);
    fi.functions[1].parent = Some("Component".into());
    fi.functions[2].parent = Some("Component".into());
    fi.functions[2].captures = vec!["local".into()];

    run_rules(&["hoistable_nested_function".into()], &mut fi);
    let violation = fi.violations.first().unwrap();
    assert_eq!(violation.count, 1);
    assert_eq!(violation.details, vec!["Component.hoistable"]);
}

#[test]
fn hoistable_nested_function_skips_tests_unless_enabled() {
    let mut skipped = mk_fi("src/component.test.tsx", &["test", "helper"], false);
    skipped.functions[1].parent = Some("test".into());
    run_rules(&["hoistable_nested_function".into()], &mut skipped);
    assert!(skipped.violations.is_empty());

    run_rules_with_test_files(&["hoistable_nested_function".into()], &mut skipped, true);
    assert_eq!(skipped.violations[0].details, vec!["test.helper"]);
}

#[test]
fn low_value_function_requires_trivial_shape_and_line_limit() {
    let mut fi = mk_fi("src/helpers.ts", &["tiny", "long", "complex"], false);
    fi.functions[0].low_value_reason = Some("thin_wrapper".into());
    fi.functions[1].low_value_reason = Some("constant_return".into());
    fi.functions[1].line_end = 10;

    run_rules_with_options(
        &["low_value_function".into()],
        &mut fi,
        RuleOptions { low_value_max_lines: 3, ..RuleOptions::default() },
    );
    let violation = fi.violations.first().unwrap();
    assert_eq!(violation.count, 1);
    assert_eq!(violation.details, vec!["tiny:thin_wrapper"]);
}

#[test]
fn one_exported_function_path_prefix_can_skip_file() {
    let fi = mk_fi("src/a.ts", &["a", "b"], false);
    let rule = OneExportedFunctionPerFile { path_prefix: Some("other/".into()) };
    assert!(rule.check(&fi).is_none());
}
