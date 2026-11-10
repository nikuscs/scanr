use std::path::Path;

use super::*;
use crate::scan::outline::OutlineView;
use crate::scan::typescript::parse::outline_source;

fn outline(source: &str) -> FileOutline {
    let view = OutlineView { signatures: true, ..OutlineView::default() };
    outline_source(Path::new("file.ts"), source).unwrap().view(view)
}

fn modified(path: &str, before: &str, after: &str) -> FileVersions {
    FileVersions {
        path: path.to_string(),
        status: FileStatus::Modified,
        from: None,
        before: Some(outline(before)),
        after: Some(outline(after)),
    }
}

/// One line per change: `file: change symbol [-> to] [@ toFile] [details]`.
fn summary(files: &[FileVersions]) -> Vec<String> {
    diff_outlines(files)
        .into_iter()
        .flat_map(|file| {
            file.changes.into_iter().map(move |change| {
                let mut line = format!(
                    "{}: {:?} {} {}",
                    file.file,
                    change.action,
                    change.kind.as_str(),
                    change.symbol
                );
                if let Some(to) = change.to {
                    line.push_str(&format!(" -> {to}"));
                }
                if let Some(to_file) = change.to_file {
                    line.push_str(&format!(" @ {to_file}"));
                }
                if !change.details.is_empty() {
                    line.push_str(&format!(" [{}]", change.details.join("; ")));
                }
                line
            })
        })
        .collect()
}

#[test]
fn reports_added_removed_and_changed_symbols() {
    let files = [modified(
        "a.ts",
        "const make = () => {\n  const keep = () => 1;\n  const legacy = () => 2;\n};\nconst value = 1;\nfunction run(id: string) {}\n",
        "export const make = () => {\n  const keep = () => 1;\n  const fresh = (n: number) => n;\n};\nfunction value() {}\nfunction run(id: string, force: boolean) {}\n",
    )];

    assert_eq!(
        summary(&files),
        [
            "a.ts: Removed function make.legacy",
            "a.ts: Added function make.fresh",
            "a.ts: Changed function make [now exported]",
            "a.ts: Changed function run [signature (id: string) → (id: string, force: boolean)]",
            "a.ts: Changed function value [const → function]",
        ]
    );
}

#[test]
fn renames_need_evidence_beyond_one_gone_one_new() {
    let files = [modified(
        "a.ts",
        "const memberships = (id: string) => 1;\nconst cache = Cache.make();\nclass Store { get() {} set() {} }\ntype Old = string;\nconst plain = 1;\n",
        "const membershipList = (id: string) => 1;\nconst store = Cache.make();\nclass Vault { get() {} set() {} drop() {} }\ntype New = string;\nconst other = 2;\n",
    )];

    assert_eq!(
        summary(&files),
        [
            // No evidence links a bare type or literal to its replacement.
            "a.ts: Removed type Old",
            "a.ts: Removed const plain",
            "a.ts: Added type New",
            "a.ts: Added method Vault.drop",
            "a.ts: Added const other",
            "a.ts: Renamed class Store -> Vault",
            "a.ts: Renamed const cache -> store",
            "a.ts: Renamed function memberships -> membershipList",
        ]
    );
}

#[test]
fn an_unrelated_replacement_is_not_a_rename() {
    let files =
        [modified("a.ts", "const load = (id: string) => 1;\n", "const save = (row: Row) => 2;\n")];

    assert_eq!(summary(&files), ["a.ts: Removed function load", "a.ts: Added function save"]);
}

#[test]
fn moves_are_tracked_within_and_across_files() {
    let files = [
        modified(
            "a.ts",
            "const make = () => {\n  const hoisted = () => 1;\n  const helper = () => 2;\n};\nfunction shared() {}\n",
            "const hoisted = () => 1;\nconst make = () => {};\n",
        ),
        FileVersions {
            path: "b.ts".to_string(),
            status: FileStatus::Added,
            from: None,
            before: None,
            after: Some(outline(
                "export function shared() {}\nconst wrap = () => {\n  const helper = () => 2;\n};\n",
            )),
        },
    ];

    assert_eq!(
        summary(&files),
        [
            "a.ts: Moved function make.helper -> wrap.helper @ b.ts",
            "a.ts: Moved function make.hoisted -> hoisted",
            "a.ts: Moved function shared -> shared @ b.ts",
            "b.ts: Added function wrap",
        ]
    );
}

#[test]
fn ambiguous_names_are_not_reported_as_moves() {
    let files = [
        modified("a.ts", "const helper = () => 1;\n", ""),
        modified("b.ts", "const helper = () => 1;\n", ""),
        modified("c.ts", "", "const helper = () => 1;\n"),
    ];

    assert_eq!(
        summary(&files),
        [
            "a.ts: Removed function helper",
            "b.ts: Removed function helper",
            "c.ts: Added function helper",
        ]
    );
}

#[test]
fn file_level_changes_are_kept_and_untouched_files_dropped() {
    let files = [
        modified("same.ts", "const a = 1;\n", "const a = 2;\n"),
        FileVersions {
            path: "gone.ts".to_string(),
            status: FileStatus::Deleted,
            from: None,
            before: Some(outline("export const dead = 1;\n")),
            after: None,
        },
        FileVersions {
            path: "new-name.ts".to_string(),
            status: FileStatus::Renamed,
            from: Some("old-name.ts".to_string()),
            before: Some(outline("const kept = 1;\n")),
            after: Some(outline("const kept = 1;\n")),
        },
    ];
    let diff = diff_outlines(&files);

    let files: Vec<_> = diff.iter().map(|file| (file.file.as_str(), file.status)).collect();
    assert_eq!(files, [("gone.ts", FileStatus::Deleted), ("new-name.ts", FileStatus::Renamed)]);
    assert_eq!(diff[1].from.as_deref(), Some("old-name.ts"));
    assert_eq!(diff[1].changes, []);
    assert_eq!(
        serde_json::to_value(&diff[0]).unwrap(),
        serde_json::json!({
            "file": "gone.ts", "status": "deleted",
            "changes": [{ "change": "removed", "symbol": "dead", "kind": "const", "unreplaced": true }]
        })
    );
}

#[test]
fn totals_count_changes_and_removals_with_nothing_beside_them() {
    let files = [modified(
        "a.ts",
        "type OldParams = string;\nconst make = () => {\n  const legacy = () => 1;\n  const list = (id: string) => 2;\n};\nconst gone = 1;\n",
        "type NewParams = string;\nconst make = () => {\n  const listAll = (page: number) => 2;\n};\n",
    )];
    let diff = diff_outlines(&files);

    let unreplaced: Vec<_> = diff[0]
        .changes
        .iter()
        .filter(|change| change.unreplaced)
        .map(|change| change.symbol.as_str())
        .collect();
    // `OldParams` and one of make's functions have a same-kind addition beside
    // them; a top-level const does not.
    assert_eq!(unreplaced, ["gone"]);
    assert_eq!(
        Totals::of(&diff),
        Totals { files: 1, added: 2, removed: 4, renamed: 0, moved: 0, changed: 0, unreplaced: 1 }
    );
}

#[test]
fn long_signatures_show_only_the_names_that_changed() {
    let short = changed_part("(id: string)", "(id: string, force: boolean)");
    assert_eq!(short, ("(id: string)".to_string(), "(id: string, force: boolean)".to_string()));

    let (old, new) = changed_part(
        "<A, I extends Schema.JsonObject, RD>({ subject, prompt, plan }: AiTypes.AiGenerateObject<A, I, RD>)",
        "<A, I extends Schema.JsonObject, RD>({ subject, prompt, plan }: AiTypes.AiGenerateObjectParams<A, I, RD>)",
    );
    assert_eq!(
        (old.as_str(), new.as_str()),
        ("…AiTypes.AiGenerateObject…", "…AiTypes.AiGenerateObjectParams…")
    );

    let (old, new) = changed_part(
        "({ page, limit, ...filter }: ActivityLogTypes.ActivityLogListParams): Effect.Effect<Page>",
        "({ page, limit, cursor, ...filter }: ActivityLogTypes.ActivityLogListParams): Effect.Effect<Page>",
    );
    assert_eq!((old.as_str(), new.as_str()), ("…...filter…", "…cursor, ...filter…"));

    let (old, new) = changed_part(
        "(params: ActivityLogTypes.ActivityLogListParams, options: ActivityLogTypes.Options)",
        "(input: ActivityLogTypes.ActivityLogListParams, options: ActivityLogTypes.Options)",
    );
    assert_eq!((old.as_str(), new.as_str()), ("…params…", "…input…"));
}
