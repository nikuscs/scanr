use std::collections::BTreeSet;

use super::*;
use crate::dependencies::Dependency;

fn dependency(
    source: &str,
    file: Option<&str>,
    package: Option<&str>,
    type_only: bool,
) -> Dependency {
    Dependency {
        source: source.to_string(),
        file: file.map(str::to_string),
        package: package.map(str::to_string),
        type_only,
    }
}

#[test]
fn renders_files_packages_unresolved_and_importers() {
    let dependencies = FileDependencies {
        imports: vec![
            dependency("./b", Some("src/b.ts"), None, false),
            dependency("./types", Some("src/a.types.ts"), None, true),
            dependency("effect/Effect", None, Some("effect"), false),
            dependency("effect/Layer", None, Some("effect"), false),
            dependency("./gone", None, None, false),
        ],
        imported_by: BTreeSet::from(["src/index.ts".to_string()]),
    };
    let leaf = FileDependencies::default();
    let cycles = Cycles {
        files: vec![vec!["src/a.ts".to_string(), "src/b.ts".to_string()]],
        packages: vec![vec!["kit-a".to_string(), "kit-b".to_string()]],
    };
    let report = DepsReport {
        version: 1,
        cycles,
        files: vec![
            DepsFile { file: "src/a.ts", dependencies: &dependencies },
            DepsFile { file: "src/leaf.ts", dependencies: &leaf },
        ],
    };

    assert_eq!(
        render(&report, false),
        "\
# Dependencies

Import cycles (type-only imports and test files ignored)
  packages  kit-a → kit-b → kit-a
  files     src/a.ts → src/b.ts → src/a.ts

src/a.ts
  imports
    src/a.types.ts  (type)
    src/b.ts
  packages     effect
  unresolved   ./gone
  imported by
    src/index.ts

src/leaf.ts
"
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["files"][0]["file"], "src/a.ts");
    assert_eq!(json["files"][0]["importedBy"], serde_json::json!(["src/index.ts"]));
    assert_eq!(json["cycles"]["files"], serde_json::json!([["src/a.ts", "src/b.ts"]]));

    let clean = DepsReport { version: 1, cycles: Cycles::default(), files: Vec::new() };
    assert_eq!(render(&clean, true), "# Dependencies\n\nNo import cycles.\n");
    assert_eq!(
        render(&clean, false),
        "# Dependencies\n\nNo import cycles.\n\nNo files in scope.\n"
    );
}

#[test]
fn scope_matches_a_file_or_a_whole_directory() {
    assert!(in_scope("src/a.ts", "src/a.ts"));
    assert!(in_scope("src/deep/a.ts", "src"));
    assert!(in_scope("src/a.ts", ""));
    assert!(!in_scope("src-other/a.ts", "src"));
    assert!(!in_scope("src/a.ts", "src/b.ts"));
}
