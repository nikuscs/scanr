use std::fs;

use tempfile::tempdir;

use super::*;
use crate::slop::{build_project_facts, collect_project_files};

fn graph(files: &[(&str, &str)]) -> DependencyGraph {
    let tmp = tempdir().expect("tempdir");
    for (path, source) in files {
        let path = tmp.path().join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        fs::write(path, source).expect("write source");
    }
    let root = tmp.path().canonicalize().expect("canonical root");
    let project = build_project_facts(&root, collect_project_files(&root).unwrap()).unwrap();
    DependencyGraph::build(&project)
}

#[test]
fn resolves_imports_to_files_packages_and_importers() {
    let graph = graph(&[
        (
            "src/events.layer.ts",
            "import * as Effect from 'effect/Effect';\nimport { db } from '@scope/kit-database/client';\nimport { Events } from './events.ts';\nimport type { Event } from './events.types';\nimport { gone } from './missing';\nexport const layer = [Effect, db, Events, gone] as Event[];\n",
        ),
        (
            "src/events.ts",
            "import type { Event } from './events.types.ts';\nexport const Events = {} as Event;\n",
        ),
        ("src/events.types.ts", "export interface Event { id: string }\n"),
        (
            "src/index.ts",
            "export { layer } from './events.layer.ts';\nexport * from './events.ts';\n",
        ),
        ("test/events.test.ts", "import { layer } from '../src/events.layer';\nexpect(layer);\n"),
    ]);

    let layer = &graph.files["src/events.layer.ts"];
    let imports: Vec<_> = layer
        .imports
        .iter()
        .map(|import| {
            (
                import.source.as_str(),
                import.file.as_deref(),
                import.package.as_deref(),
                import.type_only,
                import.is_unresolved(),
            )
        })
        .collect();
    assert_eq!(
        imports,
        [
            ("./events.ts", Some("src/events.ts"), None, false, false),
            ("./events.types", Some("src/events.types.ts"), None, true, false),
            ("./missing", None, None, false, true),
            ("@scope/kit-database/client", None, Some("@scope/kit-database"), false, false),
            ("effect/Effect", None, Some("effect"), false, false),
        ]
    );
    // Importers include re-exporting barrels and tests.
    assert_eq!(
        layer.imported_by.iter().map(String::as_str).collect::<Vec<_>>(),
        ["src/index.ts", "test/events.test.ts"]
    );
    assert_eq!(
        graph.files["src/events.types.ts"]
            .imported_by
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["src/events.layer.ts", "src/events.ts"]
    );
    assert_eq!(
        serde_json::to_value(&graph.files["src/events.ts"]).unwrap(),
        serde_json::json!({
            "imports": [{ "source": "./events.types.ts", "file": "src/events.types.ts", "typeOnly": true }],
            "importedBy": ["src/events.layer.ts", "src/index.ts"]
        })
    );
}

#[test]
fn package_names_keep_their_scope() {
    assert_eq!(package_name("effect/Effect"), "effect");
    assert_eq!(package_name("@scope/name/sub/path"), "@scope/name");
    assert_eq!(package_name("node:fs"), "node:fs");
}

#[test]
fn cycles_cover_files_and_workspace_packages_but_not_types_or_tests() {
    let graph = graph(&[
        ("packages/a/package.json", r#"{ "name": "@scope/a" }"#),
        ("packages/b/package.json", r#"{ "name": "@scope/b" }"#),
        ("packages/c/package.json", r#"{ "name": "@scope/c" }"#),
        // a → b by package name, b → a by relative path: a package cycle.
        ("packages/a/src/index.ts", "import { b } from '@scope/b';\nexport const a = b;\n"),
        (
            "packages/b/src/index.ts",
            "import { a } from '../../a/src/index.ts';\nexport const b = a;\n",
        ),
        // A file cycle inside one package.
        ("packages/c/src/one.ts", "import { two } from './two';\nexport const one = two;\n"),
        ("packages/c/src/two.ts", "import { one } from './one';\nexport const two = one;\n"),
        // Type-only and test imports do not close a loop.
        (
            "packages/c/src/model.ts",
            "import type { View } from './view';\nexport const model = {} as View;\n",
        ),
        (
            "packages/c/src/view.ts",
            "import { model } from './model';\nexport type View = typeof model;\n",
        ),
        ("packages/c/test/one.test.ts", "import { a } from '@scope/a';\nexpect(a);\n"),
        (
            "packages/a/src/helper.ts",
            "import { one } from '@scope/c';\nexport const helper = one;\n",
        ),
    ]);

    assert_eq!(
        graph.cycles(),
        Cycles {
            // `@scope/b` is not resolved to a file, so a ↔ b is a package cycle only.
            files: vec![vec![
                "packages/c/src/one.ts".to_string(),
                "packages/c/src/two.ts".to_string(),
            ]],
            packages: vec![vec!["@scope/a".to_string(), "@scope/b".to_string()]],
        }
    );
}
