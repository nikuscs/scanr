use super::*;

fn file(path: &str, role: &str, traits: &[&str]) -> RoleFile {
    RoleFile {
        path: path.to_string(),
        role: role.to_string(),
        traits: traits.iter().map(|name| ((*name).to_string(), true)).collect(),
    }
}

fn rules(entries: &[(&str, &[&str], &[&str])]) -> BTreeMap<String, RoleRule> {
    entries
        .iter()
        .map(|(role, require, forbid)| {
            let owned = |names: &[&str]| names.iter().map(|name| (*name).to_string()).collect();
            ((*role).to_string(), RoleRule { require: owned(require), forbid: owned(forbid) })
        })
        .collect()
}

#[test]
fn reports_missing_and_forbidden_traits_per_file() {
    let files = [
        file("src/a.layer.ts", ".layer", &["exported `layer`", "`make`"]),
        file("src/b.layer.ts", ".layer", &["`make`", "imports .fake"]),
        file("src/a.utils.ts", ".utils", &["exported function", "imports .layer"]),
        file("src/b.utils.ts", ".utils", &["exported function"]),
        file("src/free.ts", "(no suffix)", &["imports .fake"]),
    ];
    let rules = rules(&[
        (".layer", &["exported `layer`"], &["imports .fake"]),
        (".utils", &[], &["imports .layer", "imports .fake"]),
        (".driver", &["exported `layer`"], &[]),
    ]);

    let report = check(&files, &rules);

    assert_eq!(report.checked, 4);
    assert_eq!(
        report.violations,
        [
            Violation {
                file: "src/b.layer.ts".into(),
                role: ".layer".into(),
                missing: vec!["exported `layer`".into()],
                forbidden: vec!["imports .fake".into()],
            },
            Violation {
                file: "src/a.utils.ts".into(),
                role: ".utils".into(),
                missing: Vec::new(),
                forbidden: vec!["imports .layer".into()],
            },
        ]
    );
    assert_eq!(report.unused_rules, [".driver"]);
    assert_eq!(
        render(&report),
        "\
# Role rules

## .layer
- src/b.layer.ts
    missing    exported `layer`
    forbidden  imports .fake

## .utils
- src/a.utils.ts
    forbidden  imports .layer

Rules for roles with no files: .driver

Files breaking a rule: 2 of 4 checked.
"
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["unusedRules"], serde_json::json!([".driver"]));
    assert!(json["violations"][1].get("missing").is_none());
}

#[test]
fn a_clean_project_reports_no_breaks() {
    let files = [file("src/a.layer.ts", ".layer", &["exported `layer`"])];
    let report = check(&files, &rules(&[(".layer", &["exported `layer`"], &[])]));

    assert_eq!(report.violations, []);
    assert_eq!(render(&report), "# Role rules\n\nFiles breaking a rule: 0 of 1 checked.\n");
}

#[test]
fn only_traits_that_roles_can_print_are_accepted() {
    for name in [
        "re-exports",
        "`make`",
        "exported `layer`",
        "imports .fake",
        "imports (no suffix)",
        "exported const",
        "internal class",
        "exported const = Config.all",
        "internal function = Effect.fnUntraced",
    ] {
        assert!(is_trait(name), "{name}");
    }
    for name in [
        "import .fake",
        "imports ",
        "exports layer",
        "exported layer",
        "internal thing",
        "``",
        "exported const = ",
    ] {
        assert!(!is_trait(name), "{name}");
    }

    let typo = rules(&[(".utils", &[], &["import .layer"])]);
    let error = validate_rules(&typo).unwrap_err().to_string();
    assert!(error.contains("role `.utils`: `import .layer` is not a trait"), "{error}");
    assert!(
        validate_rules(&rules(&[(".utils", &["exported function"], &["imports .layer"])])).is_ok()
    );
}
