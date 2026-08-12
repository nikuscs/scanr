use super::*;
use crate::scan::types::{
    BindingInfo, ClassInfo, FileIndex, FunctionContent, FunctionInfo, FunctionKind,
};
use crate::slop::types::FileFacts;

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
            content: FunctionContent::Plain,
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
        classes: vec![],
        bindings,
        exports: vec![],
        violations: vec![],
        parse_errors: 0,
        slop: FileFacts::default(),
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
fn low_value_local_helper_requires_trivial_top_level_one_or_two_use_function() {
    let mut fi = mk_fi(
        "src/helpers.ts",
        &["oneUse", "twoUse", "exported", "nested", "dead", "popular", "complex", "long"],
        false,
    );
    for function in &mut fi.functions {
        function.exported = false;
        function.low_value_reason = Some("thin_wrapper".into());
    }
    fi.functions[2].exported = true;
    fi.functions[3].parent = Some("Component".into());
    fi.functions[6].low_value_reason = None;
    fi.functions[7].line_end = 4;
    fi.bindings = vec![
        ("oneUse", 1),
        ("twoUse", 2),
        ("exported", 1),
        ("nested", 1),
        ("dead", 0),
        ("popular", 3),
        ("complex", 1),
        ("long", 1),
    ]
    .into_iter()
    .map(|(name, refs)| BindingInfo {
        name: name.into(),
        kind: BindingKind::Function,
        exported: false,
        refs,
        line: 1,
        col: 1,
    })
    .collect();

    run_rules_with_options(
        &["low_value_local_helper".into()],
        &mut fi,
        RuleOptions { low_value_max_lines: 3, ..RuleOptions::default() },
    );
    let violation = fi.violations.first().unwrap();
    assert_eq!(violation.count, 2);
    assert_eq!(
        violation.details,
        vec!["oneUse:helper:thin_wrapper:1", "twoUse:helper:thin_wrapper:2"]
    );
}

#[test]
fn low_value_local_helper_matches_binding_by_declaration_line() {
    let mut fi = mk_fi("src/helpers.ts", &["helper"], false);
    fi.functions[0].exported = false;
    fi.functions[0].low_value_reason = Some("property_return".into());
    fi.functions[0].line = 10;
    fi.functions[0].line_end = 12;
    fi.bindings = vec![
        BindingInfo {
            name: "helper".into(),
            kind: BindingKind::Function,
            exported: false,
            refs: 8,
            line: 2,
            col: 1,
        },
        BindingInfo {
            name: "helper".into(),
            kind: BindingKind::Function,
            exported: false,
            refs: 2,
            line: 10,
            col: 1,
        },
    ];

    run_rules(&["low_value_local_helper".into()], &mut fi);
    assert_eq!(fi.violations[0].details, vec!["helper:helper:property_return:2"]);
}

#[test]
fn dominant_function_with_two_tiny_helpers_is_grouped() {
    let mut fi = mk_fi("src/feature.ts", &["large", "first", "second"], false);
    for function in &mut fi.functions {
        function.exported = false;
    }
    fi.functions[0].line_end = 300;
    fi.functions[1].low_value_reason = Some("thin_wrapper".into());
    fi.functions[2].low_value_reason = None;
    fi.bindings = vec![
        BindingInfo {
            name: "first".into(),
            kind: BindingKind::Function,
            exported: false,
            refs: 1,
            line: 1,
            col: 1,
        },
        BindingInfo {
            name: "second".into(),
            kind: BindingKind::Function,
            exported: false,
            refs: 2,
            line: 1,
            col: 1,
        },
    ];

    run_rules(&["dominant_container_tiny_helpers".into()], &mut fi);
    let violation = fi.violations.first().unwrap();
    assert_eq!(violation.count, 2);
    assert_eq!(violation.details[0], "container:function:large:300");
    assert_eq!(violation.details[1], "helper:first:thin_wrapper:1:1");
    assert_eq!(violation.details[2], "helper:second:small_local:2:1");
}

#[test]
fn dominant_class_requires_configured_tiny_helper_count() {
    let mut fi = mk_fi("src/feature.ts", &["onlyHelper"], false);
    fi.functions[0].exported = false;
    fi.functions[0].low_value_reason = Some("thin_wrapper".into());
    fi.classes =
        vec![ClassInfo { name: "LargeService".into(), exported: true, line: 1, line_end: 350 }];
    fi.bindings = vec![BindingInfo {
        name: "onlyHelper".into(),
        kind: BindingKind::Function,
        exported: false,
        refs: 1,
        line: 1,
        col: 1,
    }];

    run_rules(&["dominant_container_tiny_helpers".into()], &mut fi);
    assert!(fi.violations.is_empty());

    fi.functions.push(FunctionInfo {
        name: Some("secondHelper".into()),
        parent: None,
        captures: vec![],
        low_value_reason: Some("constant_return".into()),
        kind: FunctionKind::Declaration,
        exported: false,
        is_async: false,
        is_generator: false,
        content: FunctionContent::Plain,
        line: 10,
        col: 1,
        line_end: 12,
    });
    fi.bindings.push(BindingInfo {
        name: "secondHelper".into(),
        kind: BindingKind::Function,
        exported: false,
        refs: 2,
        line: 10,
        col: 1,
    });
    run_rules(&["dominant_container_tiny_helpers".into()], &mut fi);
    assert_eq!(fi.violations[0].details[0], "container:class:LargeService:350");
}

#[test]
fn low_value_local_helper_excludes_tsx_components_and_hooks() {
    let mut fi = mk_fi("src/view.tsx", &["Card", "useThing", "formatValue"], false);
    for function in &mut fi.functions {
        function.exported = false;
        function.low_value_reason = Some("thin_wrapper".into());
        function.content = if function.name.as_deref() == Some("Card") {
            FunctionContent::Jsx
        } else {
            FunctionContent::Plain
        };
    }
    fi.bindings = ["Card", "useThing", "formatValue"]
        .into_iter()
        .map(|name| BindingInfo {
            name: name.into(),
            kind: BindingKind::Function,
            exported: false,
            refs: 1,
            line: 1,
            col: 1,
        })
        .collect();

    run_rules(&["low_value_local_helper".into()], &mut fi);
    assert_eq!(fi.violations[0].details, vec!["formatValue:helper:thin_wrapper:1"]);
}

#[test]
fn loose_low_value_includes_small_ordinary_and_component_local_bodies() {
    let mut fi = mk_fi(
        "src/view.tsx",
        &["formatValue", "handleClick", "Card", "useThing", "manyUses"],
        false,
    );
    for function in &mut fi.functions {
        function.exported = false;
        function.low_value_reason = None;
        function.line_end = 3;
    }
    fi.functions[1].parent = Some("Card".into());
    fi.functions[2].content = FunctionContent::Jsx;
    fi.bindings = ["formatValue", "handleClick", "Card", "useThing", "manyUses"]
        .into_iter()
        .map(|name| BindingInfo {
            name: name.into(),
            kind: BindingKind::Function,
            exported: false,
            refs: if name == "manyUses" { 3 } else { 1 },
            line: 1,
            col: 1,
        })
        .collect();

    run_rules_with_options(&["low_value_local_helper".into()], &mut fi, RuleOptions::default());
    assert!(fi.violations.is_empty());

    run_rules_with_options(
        &["low_value_local_helper".into()],
        &mut fi,
        RuleOptions { loose_low_value: true, ..RuleOptions::default() },
    );
    assert_eq!(
        fi.violations[0].details,
        vec!["formatValue:helper:small_local:1", "handleClick:componentLocal:small_local:1"]
    );
}

#[test]
fn loose_dominant_rule_includes_component_local_satellites() {
    let mut fi = mk_fi("src/view.tsx", &["BigView", "first", "second"], false);
    for function in &mut fi.functions {
        function.exported = false;
    }
    fi.functions[0].line_end = 300;
    fi.functions[0].content = FunctionContent::Jsx;
    fi.functions[1].parent = Some("BigView".into());
    fi.functions[2].parent = Some("BigView".into());
    fi.bindings = ["first", "second"]
        .into_iter()
        .map(|name| BindingInfo {
            name: name.into(),
            kind: BindingKind::Function,
            exported: false,
            refs: 1,
            line: 1,
            col: 1,
        })
        .collect();

    run_rules_with_options(
        &["dominant_container_tiny_helpers".into()],
        &mut fi,
        RuleOptions::default(),
    );
    assert!(fi.violations.is_empty());

    run_rules_with_options(
        &["dominant_container_tiny_helpers".into()],
        &mut fi,
        RuleOptions { loose_low_value: true, ..RuleOptions::default() },
    );
    assert_eq!(fi.violations[0].count, 2);
}

#[test]
fn satellite_cluster_groups_multiple_low_use_helpers_once() {
    let mut fi = mk_fi("src/helpers.ts", &["first", "second", "manyUses"], false);
    for function in &mut fi.functions {
        function.exported = false;
        function.line_end = function.line + 5;
    }
    fi.functions[0].low_value_reason = Some("thin_wrapper".into());
    fi.bindings = [("first", 2usize), ("second", 1usize), ("manyUses", 3usize)]
        .into_iter()
        .map(|(name, refs)| BindingInfo {
            name: name.into(),
            kind: BindingKind::Function,
            exported: false,
            refs,
            line: 1,
            col: 1,
        })
        .collect();

    run_rules(&["satellite_cluster".into()], &mut fi);
    assert_eq!(fi.violations.len(), 1);
    assert_eq!(fi.violations[0].count, 2);
    assert_eq!(
        fi.violations[0].details,
        vec!["helper:first:thin_wrapper:2:6", "helper:second:single_use:1:6"]
    );
}

#[test]
fn satellite_cluster_requires_multiple_members_and_skips_nested_functions() {
    let mut fi = mk_fi("src/helpers.ts", &["only", "nested"], false);
    for function in &mut fi.functions {
        function.exported = false;
    }
    fi.functions[1].parent = Some("owner".into());
    fi.bindings = ["only", "nested"]
        .into_iter()
        .map(|name| BindingInfo {
            name: name.into(),
            kind: BindingKind::Function,
            exported: false,
            refs: 1,
            line: 1,
            col: 1,
        })
        .collect();

    run_rules(&["satellite_cluster".into()], &mut fi);
    assert!(fi.violations.is_empty());
}

#[test]
fn one_exported_function_path_prefix_can_skip_file() {
    let fi = mk_fi("src/a.ts", &["a", "b"], false);
    let rule = OneExportedFunctionPerFile { path_prefix: Some("other/".into()) };
    assert!(rule.check(&fi).is_none());
}
