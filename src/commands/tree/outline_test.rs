use std::fs;

use tempfile::tempdir;

use super::*;
use crate::commands::tree::build_node;

fn render(files: &[(&str, &str)], include_tests: bool, deep: bool, max_depth: usize) -> String {
    render_view(files, include_tests, OutlineView { deep, ..OutlineView::default() }, max_depth)
}

fn render_view(
    files: &[(&str, &str)],
    include_tests: bool,
    view: OutlineView,
    max_depth: usize,
) -> String {
    let tmp = tempdir().expect("tempdir");
    for (path, source) in files {
        let path = tmp.path().join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        fs::write(path, source).expect("write source");
    }
    let root = tmp.path().canonicalize().expect("canonical root");
    let outlines = collect_outlines(&root, &root, include_tests, view).expect("outlines");
    let tree = build_node(&root, &root, include_tests, &BTreeMap::new()).expect("tree");
    let mut lines = Vec::new();
    render_outline_tree(&tree, &outlines, &mut lines, max_depth, view.deep);
    lines.join("\n")
}

const LAYER: &str = r"import * as Effect from 'effect/Effect';
const make = Effect.gen(function* () {
  const db = yield* Database;
  const record = Effect.fn('record')(function* (event: Event) {
    const tokens = 1;
  });
  return { record };
});
export const layer = Layer.effect(Service, make);
";

const INDEX: &str = r"export { layer } from './layer.ts';
export * from './errors.ts';
export * as types from './types.ts';
export type { Shape } from './types.ts';
";

const SERVICE: &str = r"export class Service extends Base { run() {} limit = 1; }
export default function main() {}
export interface Shape { side: number }
let counter = 0;
";

#[test]
fn renders_nested_declarations_with_aligned_columns() {
    let output = render(
        &[("src/layer.ts", LAYER), ("src/index.ts", INDEX), ("src/service.ts", SERVICE)],
        false,
        false,
        6,
    );

    assert_eq!(
        output,
        "\
└── src/
    ├── index.ts
    │   ├── layer       re-exported from ./layer.ts
    │   ├── *           re-exported from ./errors.ts
    │   ├── * as types  re-exported from ./types.ts
    │   └── Shape       re-exported as type from ./types.ts
    ├── layer.ts
    │   ├── make                    (line 2)
    │   │   ├── db
    │   │   └── record()            (line 4)
    │   └── layer         exported  (line 9)
    └── service.ts
        ├── Service    class  exported      (line 1)
        │   └── run()                       (line 1)
        ├── main()     exported as default  (line 2)
        ├── Shape      interface  exported  (line 3)
        └── counter    let                  (line 4)"
    );
}

#[test]
fn deep_adds_locals_members_callees_and_imports() {
    let output = render(&[("layer.ts", LAYER), ("service.ts", SERVICE)], false, true, 6);

    assert_eq!(
        output,
        "\
├── layer.ts
│   ├── imports                   effect/Effect
│   ├── make = Effect.gen                        (line 2)
│   │   ├── db
│   │   └── record() = Effect.fn                 (line 4)
│   │       └── tokens
│   └── layer = Layer.effect      exported       (line 9)
└── service.ts
    ├── Service    class  extends Base  exported  (line 1)
    │   ├── run()                                 (line 1)
    │   └── limit
    ├── main()     exported as default            (line 2)
    ├── Shape      interface  exported            (line 3)
    │   └── side
    └── counter    let                            (line 4)"
    );
}

#[test]
fn lists_unscanned_files_and_skips_tests_by_default() {
    let files = [
        ("package.json", "{}"),
        ("src/a.ts", "export const a = 1;\n"),
        ("src/a.test.ts", "const fixture = 1;\n"),
        ("src/deep/nested/b.ts", "export const b = 1;\n"),
    ];

    let output = render(&files, false, false, 6);
    assert!(output.contains("└── package.json"), "output: {output}");
    assert!(output.contains("a.test.ts\n"), "output: {output}");
    assert!(!output.contains("fixture"), "output: {output}");
    assert!(output.contains("b  exported  (line 1)"), "output: {output}");

    let with_tests = render(&files, true, false, 6);
    assert!(with_tests.contains("fixture"), "output: {with_tests}");

    let shallow = render(&files, false, false, 2);
    assert!(shallow.contains("deep/ (1d 1f)"), "output: {shallow}");
    assert!(!shallow.contains("b.ts"), "output: {shallow}");
}

#[test]
fn shows_interface_methods_and_functions_exposed_under_another_key() {
    let output = render(
        &[(
            "store.layer.ts",
            "interface Store { webhookList: () => void; size: number }\nconst make = () => {\n  const webhooks = () => {};\n  return Store.of({ webhookList: webhooks });\n};\n",
        )],
        false,
        false,
        6,
    );

    assert_eq!(
        output,
        "\
└── store.layer.ts
    ├── Store              interface               (line 1)
    │   └── webhookList()                          (line 1)
    └── make()                                     (line 2)
        └── webhooks()     exposed as webhookList  (line 3)"
    );
}
