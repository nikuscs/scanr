use std::fs;
use tempfile::tempdir;

use super::types::{ProjectFacts, SlopFinding, SlopKind, SlopOptions};
use super::{build_project_facts, collect_project_files, module_detectors};

fn project(files: &[(&str, &str)]) -> ProjectFacts {
    let root = tempdir().unwrap();
    for (path, source) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let root = root.path().canonicalize().unwrap();
    build_project_facts(&root, collect_project_files(&root).unwrap()).unwrap()
}

fn findings(project: &ProjectFacts, kind: SlopKind, options: &SlopOptions) -> Vec<SlopFinding> {
    module_detectors::detectors(options)
        .into_iter()
        .find(|detector| detector.kind() == kind)
        .unwrap()
        .detect(project)
}

#[test]
fn pass_through_exports_and_wrappers_require_no_own_logic() {
    let positive = [
        "export * from './source';",
        "import { run } from './source'; const api = { run }; export { api };",
        "import { run } from './source'; const api = { run }; export default api;",
        "import { run } from './source'; export const execute = (...args: string[]) => run(...args);",
        "import * as api from './source'; export const execute = (value: string) => api.run(value);",
        "export { run as execute } from './source';",
        "import { run } from './source'; export { run };",
        "import { run } from './source'; export default run;",
        "import { run } from './source'; export const execute = (value: string) => run(value);",
        "import { run } from './source'; export function execute(value: string) { return run(value); }",
        "import { run } from './source'; export const api = { execute: (value: string) => run(value) };",
        "import { run } from './source'; export const create = () => ({ execute: (value: string) => run(value) });",
    ];
    for source in positive {
        let project = project(&[("wrapper.ts", source)]);
        assert_eq!(
            findings(&project, SlopKind::PassThroughModule, &SlopOptions::default()).len(),
            1,
            "{source}"
        );
    }
    let negative = [
        "import { run } from './source';",
        "export const value = 1;",
        "export {};",
        "export {} from './source';",
        "const value = sideEffect(); export { value };",
        "import { run } from './source'; export const execute = (...args: string[]) => run(args);",
        "import { run } from './source'; export const execute = (a: string, ...args: string[]) => run(...args, a);",
        "export * from './source'; console.log('side effect');",
        "import './setup'; export * from './source';",
        "import { run } from './source'; export const execute = (value: string) => run(value.trim());",
        "import { run } from './source'; export const execute = (a: string, b: string) => run(b, a);",
        "import { run } from './source'; export const execute = (run: Function) => run(run);",
        "import { run } from './source'; export const execute = (value = sideEffect()) => run(value);",
        "import { run } from './source'; export const api = { execute: run, own: 1 };",
        "import { run } from './source'; export function execute(value: string) { console.log(value); return run(value); }",
        "import { run } from './source'; export const execute = async (value: string) => run(value);",
    ];
    for source in negative {
        let project = project(&[("wrapper.ts", source)]);
        assert!(
            findings(&project, SlopKind::PassThroughModule, &SlopOptions::default()).is_empty(),
            "{source}"
        );
    }
}

#[test]
fn oversplit_resolves_dotted_modules_and_nearest_package_aliases() {
    let project = project(&[
        (
            "apps/api/tsconfig.json",
            r##"{"compilerOptions":{"paths":{"#/*":["./src/*"],"#tiny":["./src/tiny.constants.ts"]}}}"##,
        ),
        ("apps/api/src/tiny.constants.ts", "export const a = 1;"),
        ("apps/api/src/first.ts", "import { a } from './tiny.constants';"),
        ("apps/api/src/second.ts", "import { a } from '#/tiny.constants';"),
        ("apps/api/src/third.ts", "import { a } from '#tiny';"),
    ]);
    assert_eq!(project.module_importers["apps/api/src/tiny.constants.ts"].len(), 3);
    assert_eq!(
        findings(&project, SlopKind::OversplitModule, &SlopOptions::default()),
        [] as [crate::slop::types::SlopFinding; 0]
    );
}

#[test]
fn oversplit_counts_unique_non_test_importers_and_names_the_consumer() {
    let project = project(&[
        ("tiny.ts", "export const a = 1; export const b = 2;"),
        (
            "consumer.ts",
            "import { a } from './tiny'; import { b } from './tiny'; console.log(a, b);",
        ),
        ("tiny.test.ts", "import { a } from './tiny'; console.log(a);"),
    ]);
    let hits = findings(&project, SlopKind::OversplitModule, &SlopOptions::default());
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].span.path, "tiny.ts");
    assert!(hits[0].evidence[0].detail.contains("consumer.ts"));
}

#[test]
fn oversplit_counts_reexports_namespace_and_static_dynamic_imports() {
    for consumer in [
        "export { a } from './tiny';",
        "export * from './tiny';",
        "import * as tiny from './tiny'; console.log(tiny);",
        "const tiny = import('./tiny');",
    ] {
        let project = project(&[("tiny.ts", "export const a = 1;"), ("consumer.ts", consumer)]);
        assert_eq!(
            findings(&project, SlopKind::OversplitModule, &SlopOptions::default()).len(),
            1,
            "{consumer}"
        );
    }
    for second in [
        "import { a } from './tiny'; console.log(a);",
        "export * from './tiny';",
        "import('./tiny');",
    ] {
        let project = project(&[
            ("tiny.ts", "export const a = 1;"),
            ("first.ts", "import { a } from './tiny'; console.log(a);"),
            ("second.ts", second),
        ]);
        assert_eq!(
            findings(&project, SlopKind::OversplitModule, &SlopOptions::default()),
            [] as [crate::slop::types::SlopFinding; 0]
        );
    }
}

#[test]
fn oversplit_requires_exports_and_a_consumer_not_side_effects() {
    for files in [
        vec![("tiny.ts", "export const a = 1;")],
        vec![
            ("tiny.ts", "export const a = 1;"),
            ("consumer.test.ts", "import { a } from './tiny';"),
        ],
        vec![("tiny.ts", "export const a = 1;"), ("consumer.ts", "import './tiny';")],
        vec![("tiny.ts", "const a = 1;"), ("consumer.ts", "import * as tiny from './tiny';")],
    ] {
        assert_eq!(
            findings(&project(&files), SlopKind::OversplitModule, &SlopOptions::default()),
            [] as [crate::slop::types::SlopFinding; 0]
        );
    }
}

#[test]
fn oversplit_ignores_generated_importers_but_keeps_real_consumers() {
    for (path, source) in [
        ("routes.generated.ts", "import { a } from './tiny';"),
        (
            "routes.tree.ts",
            "/* eslint-disable */\n\n// @ts-nocheck\n\n// noinspection JSUnusedGlobalSymbols\n\n// This file was automatically generated by a router.\nimport { a } from './tiny';",
        ),
        ("generated.ts", "// @generated\nimport { a } from './tiny';"),
    ] {
        let files = [("tiny.ts", "export const a = 1;"), (path, source)];
        let generated_only = project(&files);
        assert!(generated_only.files[path].is_generated);
        assert_eq!(
            findings(&generated_only, SlopKind::OversplitModule, &SlopOptions::default()),
            [] as [crate::slop::types::SlopFinding; 0]
        );
        let mut files = files.to_vec();
        files.push(("consumer.ts", "import { a } from './tiny';"));
        let with_consumer = project(&files);
        let hits = findings(&with_consumer, SlopKind::OversplitModule, &SlopOptions::default());
        assert_eq!(hits.len(), 1);
        assert!(hits[0].evidence[0].detail.contains("consumer.ts"));
        assert_eq!(with_consumer.module_importers["tiny.ts"].len(), 1);
    }
}

#[test]
fn oversplit_line_limit_is_inclusive_and_counts_comments_and_blank_lines() {
    let source = format!("// comment\n{}export const a = 1;\n", "\n".repeat(28));
    let project = project(&[("tiny.ts", &source), ("consumer.ts", "import { a } from './tiny';")]);
    let mut options = SlopOptions::default();
    assert_eq!(findings(&project, SlopKind::OversplitModule, &options).len(), 1);
    options.oversplit_max_lines = 29;
    assert_eq!(
        findings(&project, SlopKind::OversplitModule, &options),
        [] as [crate::slop::types::SlopFinding; 0]
    );
}

#[test]
fn importer_globs_exempt_only_matching_consumers() {
    let project = project(&[
        ("tiny.ts", "export const a = 1;"),
        ("domain/app.service.ts", "import { a } from '../tiny';"),
        ("rpc.ts", "export const b = 1;"),
        ("rpc.router.ts", "import { b } from './rpc';"),
        ("plugin.ts", "export const c = 1;"),
        ("vite.config.ts", "import { c } from './plugin';"),
    ]);
    let mut builder = ignore::gitignore::GitignoreBuilder::new("");
    for pattern in ["*.service.ts", "rpc.router.ts", "vite.config.ts"] {
        builder.add_line(None, pattern).unwrap();
    }
    let options = SlopOptions {
        oversplit_allow_importer: builder.build().unwrap(),
        ..SlopOptions::default()
    };
    assert_eq!(
        findings(&project, SlopKind::OversplitModule, &options),
        [] as [crate::slop::types::SlopFinding; 0]
    );
    assert_eq!(findings(&project, SlopKind::OversplitModule, &SlopOptions::default()).len(), 3);
}

#[test]
fn module_detectors_skip_tests_generated_declarations_and_entrypoints() {
    for path in ["tiny.test.ts", "tiny.d.ts", "tiny.generated.ts", "index.ts"] {
        let source = "export { a } from './source';";
        let consumer = format!("export * from './{path}';");
        let project = project(&[(path, source), ("consumer.ts", &consumer)]);
        for kind in [SlopKind::OversplitModule, SlopKind::PassThroughModule] {
            assert!(
                findings(&project, kind, &SlopOptions::default())
                    .iter()
                    .all(|hit| hit.span.path != path),
                "{path}"
            );
        }
    }
    for source in ["// @generated\nexport { a } from './source';", "export const = ;"] {
        let project = project(&[("tiny.ts", source), ("consumer.ts", "export * from './tiny';")]);
        for kind in [SlopKind::OversplitModule, SlopKind::PassThroughModule] {
            assert!(
                findings(&project, kind, &SlopOptions::default())
                    .iter()
                    .all(|hit| hit.span.path != "tiny.ts")
            );
        }
    }
}
