use std::path::Path;

use super::*;
use crate::scan::typescript::parse::outline_source;

const THRESHOLDS: Thresholds = Thresholds { min_files: 5, usual: 80, unusual: 10 };

fn file(path: &str, source: &str) -> RoleFile {
    let outline = outline_source(Path::new(path), source).unwrap();
    RoleFile { path: path.to_string(), role: file_role(path), traits: file_traits(&outline, None) }
}

const LAYER: &str =
    "const make = Effect.gen(function* () {});\nexport const layer = Layer.effect(Tag, make);\n";

#[test]
fn roles_come_from_the_file_name_suffix() {
    assert_eq!(file_role("packages/kit/src/events.layer.ts"), ".layer");
    assert_eq!(file_role("src/events-tokenlens.adapter.tsx"), ".adapter");
    assert_eq!(file_role("src/migrations/2026-create-tables.sqlite.ts"), ".sqlite");
    assert_eq!(file_role("src/index.ts"), "index");
    assert_eq!(file_role("src/events.ts"), "(no suffix)");
    assert_eq!(file_role("src/assets.d.ts"), "(no suffix)");
    assert_eq!(file_role("src/storage.assets.d.ts"), ".assets");
    assert_eq!(file_role("test/events.fake.test.ts"), "test");
}

#[test]
fn traits_cover_names_kinds_producers_and_imported_roles() {
    let outline = outline_source(
        Path::new("a.layer.ts"),
        "const make = Effect.gen(function* () {});\nexport const layer = Layer.effect(Tag, make);\nexport class Tag {}\nexport * from './b';\n",
    )
    .unwrap();
    let dependencies = FileDependencies {
        imports: vec![
            crate::dependencies::Dependency {
                source: "./a.types".into(),
                file: Some("src/a.types.ts".into()),
                package: None,
                type_only: true,
            },
            crate::dependencies::Dependency {
                source: "effect".into(),
                file: None,
                package: Some("effect".into()),
                type_only: false,
            },
        ],
        ..FileDependencies::default()
    };
    let traits = file_traits(&outline, Some(&dependencies));

    let names: Vec<_> =
        traits.iter().map(|(name, distinctive)| (name.as_str(), *distinctive)).collect();
    assert_eq!(
        names,
        [
            ("`make`", false),
            ("exported `Tag`", false),
            ("exported `layer`", false),
            ("exported class", true),
            ("exported const", true),
            ("exported const = Layer.effect", false),
            ("imports .types", true),
            ("internal const", true),
            ("internal const = Effect.gen", false),
            ("re-exports", true),
        ]
    );
}

#[test]
fn reports_files_that_miss_a_usual_trait_or_have_a_rare_one() {
    let mut files: Vec<RoleFile> =
        (0..10).map(|index| file(&format!("src/m{index}.layer.ts"), LAYER)).collect();
    files.push(file("src/no-make.layer.ts", "export const layer = Layer.succeed(Tag, {});\n"));
    files.push(file(
        "src/extra.layer.ts",
        "const make = Effect.gen(function* () {});\nexport const layer = Layer.effect(Tag, make);\nclass Helper {}\n",
    ));
    files.push(file("src/only.worker.ts", "export const worker = 1;\n"));

    let report = build_report(&files, THRESHOLDS);

    assert_eq!(report.roles.len(), 1);
    let layer = &report.roles[0];
    assert_eq!((layer.name.as_str(), layer.files), (".layer", 12));
    let usual: Vec<_> =
        layer.usual.iter().map(|usual| (usual.name.as_str(), usual.files)).collect();
    assert_eq!(
        usual,
        [
            ("exported `layer`", 12),
            ("exported const", 12),
            ("`make`", 11),
            ("exported const = Layer.effect", 11),
            ("internal const", 11),
            ("internal const = Effect.gen", 11),
        ]
    );
    assert_eq!(
        layer.outliers,
        [
            Outlier {
                file: "src/extra.layer.ts".into(),
                missing: Vec::new(),
                unusual: vec![TraitCount { name: "internal class".into(), files: 1 }],
            },
            Outlier {
                file: "src/no-make.layer.ts".into(),
                missing: vec![
                    "`make`".into(),
                    "exported const = Layer.effect".into(),
                    "internal const".into(),
                    "internal const = Effect.gen".into(),
                ],
                unusual: Vec::new(),
            },
        ]
    );
    assert_eq!(
        report.small_roles,
        [SmallRole { name: ".worker".into(), files: vec!["src/only.worker.ts".into()] }]
    );

    let text = render(&report);
    assert!(text.contains("## .layer — 12 files\nUsually: exported `layer` (12), "), "{text}");
    assert!(
        text.contains("- src/extra.layer.ts\n    unusual  internal class (1 of 12)\n"),
        "{text}"
    );
    assert!(text.contains("- src/no-make.layer.ts\n    missing  `make`, "), "{text}");
    assert!(
        text.contains("## Roles with too few files to compare\n- .worker — src/only.worker.ts\n"),
        "{text}"
    );
}

#[test]
fn a_group_that_shares_nothing_has_no_odd_ones_out() {
    let files: Vec<RoleFile> = (0..12)
        .map(|index| {
            let source = if index == 0 {
                "class Odd {}\n".to_string()
            } else {
                format!("export function f{index}() {{}}\nexport const c{index} = {index};\n")
                    .split('\n')
                    .nth(index % 2)
                    .unwrap()
                    .to_string()
            };
            file(&format!("src/m{index}.ts"), &source)
        })
        .collect();

    let report = build_report(&files, THRESHOLDS);

    assert_eq!(report.roles[0].name, "(no suffix)");
    assert_eq!(report.roles[0].usual, []);
    assert_eq!(report.roles[0].outliers, []);
    assert!(render(&report).contains("No shared pattern.\nNo odd ones out.\n"));
}
