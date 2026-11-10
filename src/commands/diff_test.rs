use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use super::*;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git").args(args).current_dir(root).output().expect("run git");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn report(root: &Path, deep: bool) -> DiffReport {
    let base = resolve_base(root, "HEAD").expect("base");
    let view = OutlineView { deep, signatures: true, exported: false };
    let files = changed_files(&base)
        .expect("changed files")
        .into_iter()
        .map(|file| load_versions(&base, file, view))
        .collect::<Result<Vec<_>>>()
        .expect("versions");
    DiffReport::new("HEAD".to_string(), base.commit, diff_outlines(&files))
}

fn repository() -> tempfile::TempDir {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    fs::write(
        root.join("layer.ts"),
        "const make = () => {\n  const record = (event: Event) => 1;\n  const helper = () => 2;\n};\nexport const layer = make;\n",
    )
    .unwrap();
    fs::write(root.join("old.ts"), "export const legacy = 1;\n").unwrap();
    fs::write(root.join("moved.ts"), "export const kept = 1;\n").unwrap();
    fs::write(root.join("notes.md"), "# notes\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "base"]);
    tmp
}

#[test]
fn compares_the_working_tree_with_the_base_revision() {
    let tmp = repository();
    let root = tmp.path();
    fs::write(
        root.join("layer.ts"),
        "const make = () => {\n  const record = (event: Event, force: boolean) => 1;\n};\nexport const layer = make;\n",
    )
    .unwrap();
    fs::write(root.join("utils.ts"), "export const helper = () => 2;\n").unwrap();
    fs::remove_file(root.join("old.ts")).unwrap();
    git(root, &["mv", "moved.ts", "renamed.ts"]);
    fs::write(root.join("notes.md"), "# changed\n").unwrap();

    let text = render(&report(root, false));
    let (header, body) = text.split_once('\n').unwrap();
    assert!(header.starts_with("# Outline diff against HEAD ("), "header: {header}");
    assert_eq!(
        body,
        "
4 files: 0 added, 1 removed, 0 renamed, 1 moved, 1 changed

Removed with nothing of the same kind added beside it (1):
  old.ts  legacy

layer.ts
  > make.helper() → helper()  moved to utils.ts
  ~ make.record()             signature (event: Event) → (event: Event, force: boolean)

old.ts  (deleted)
  - legacy  removed

renamed.ts  (renamed from moved.ts)

utils.ts  (new file)
"
    );
}

#[test]
fn unchanged_trees_and_non_code_files_report_nothing() {
    let tmp = repository();
    fs::write(tmp.path().join("notes.md"), "# changed\n").unwrap();

    let report = report(tmp.path(), false);
    assert_eq!(report.files, []);
    assert!(render(&report).ends_with("\nNo declaration changes.\n"));
}

#[test]
fn deep_compares_locals_too() {
    let tmp = repository();
    let root = tmp.path();
    fs::write(
        root.join("layer.ts"),
        "const make = () => {\n  const record = (event: Event) => {\n    const tokens = 1;\n  };\n  const helper = () => 2;\n};\nexport const layer = make;\n",
    )
    .unwrap();

    assert_eq!(report(root, false).files, []);
    let deep = report(root, true);
    assert_eq!(deep.files[0].changes[0].symbol, "make.record.tokens");
    assert_eq!(deep.files[0].changes[0].action, ChangeKind::Added);

    let json = serde_json::to_value(&deep).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["totals"]["added"], 1);
    assert_eq!(json["files"][0]["status"], "modified");
    assert_eq!(json["files"][0]["changes"][0]["kind"], "const");
}

#[test]
fn name_status_covers_every_change_kind() {
    let files = parse_name_status(
        b"M\0a.ts\0A\0b.ts\0D\0c.ts\0R090\0old.ts\0new.ts\0C100\0src.ts\0copy.ts\0",
    )
    .unwrap();
    let summary: Vec<_> =
        files.iter().map(|file| (file.path.as_str(), file.status, file.from.as_deref())).collect();
    assert_eq!(
        summary,
        [
            ("a.ts", FileStatus::Modified, None),
            ("b.ts", FileStatus::Added, None),
            ("c.ts", FileStatus::Deleted, None),
            ("new.ts", FileStatus::Renamed, Some("old.ts")),
            ("copy.ts", FileStatus::Added, None),
        ]
    );
    assert!(parse_name_status(b"R100\0only-one.ts\0").is_err());
}
