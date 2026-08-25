use std::fs;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};

use assert_cmd::Command;
use tempfile::tempdir;

fn js_runtime_available() -> bool {
    ["bun", "node"].iter().any(|candidate| {
        StdCommand::new(candidate)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .is_ok_and(|status| status.success())
    })
}

fn write_fixture(root: &Path) {
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"jsx":"react-jsx"},"include":["*.tsx","*.ts"]}"#,
    )
    .unwrap();
    fs::write(root.join("card.tsx"), "export function Card() { return <div />; }\n").unwrap();
    fs::write(
        root.join("app.tsx"),
        "import { Card } from \"./card\";\nexport const App = () => <Card />;\n",
    )
    .unwrap();
    fs::write(root.join("notes.ts"), "const label = \"Card\";\n").unwrap();
}

#[test]
fn rename_rewrites_declaration_import_and_jsx() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    write_fixture(root.path());
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("card.tsx"), "{stdout}");
    assert!(stdout.contains("app.tsx"), "{stdout}");
    assert!(stdout.contains("notes.ts"), "{stdout}");
    assert!(stdout.contains("leftovers"), "{stdout}");

    let card = fs::read_to_string(root.path().join("card.tsx")).unwrap();
    let app = fs::read_to_string(root.path().join("app.tsx")).unwrap();
    assert!(card.contains("ProfileCard"), "{card}");
    assert!(!card.contains("function Card"), "{card}");
    assert!(app.contains("ProfileCard"), "{app}");
    assert!(!app.contains("{ Card"), "{app}");
    assert!(!app.contains("<Card"), "{app}");
}

#[test]
fn rename_rejects_ambiguous_bare_name() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    write_fixture(root.path());
    fs::write(root.path().join("notes.ts"), "const label = \"Card\";\nexport const Card = 1;\n")
        .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("#Card"), "{stderr}");
    assert!(stderr.contains("card.tsx#Card"), "{stderr}");
    assert!(stderr.contains("notes.ts#Card"), "{stderr}");
}

#[test]
fn rename_qualified_target_resolves_ambiguity() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    write_fixture(root.path());
    fs::write(root.path().join("notes.ts"), "const label = \"Card\";\nexport const Card = 1;\n")
        .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.tsx#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let notes = fs::read_to_string(root.path().join("notes.ts")).unwrap();
    assert!(notes.contains("export const Card = 1;"), "{notes}");
    let card = fs::read_to_string(root.path().join("card.tsx")).unwrap();
    assert!(card.contains("ProfileCard"), "{card}");
}

#[test]
fn rename_keeps_import_alias_local_name() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("use.ts"),
        "import { Card as LocalCard } from \"./card\";\nexport const run = () => LocalCard();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let usage = fs::read_to_string(root.path().join("use.ts")).unwrap();
    assert!(usage.contains("ProfileCard as LocalCard"), "{usage}");
    assert!(usage.contains("LocalCard()"), "{usage}");
}

#[test]
fn rename_follows_barrel_reexport_chain() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(root.path().join("index.ts"), "export { Card } from \"./card\";\n").unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import { Card } from \"./index\";\nexport const run = () => Card();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let card = fs::read_to_string(root.path().join("card.ts")).unwrap();
    let barrel = fs::read_to_string(root.path().join("index.ts")).unwrap();
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(card.contains("export function ProfileCard()"), "{card}");
    assert!(barrel.contains("export { ProfileCard as Card }"), "{barrel}");
    assert!(app.contains("import { Card }"), "{app}");
    assert!(app.contains("Card()"), "{app}");
}

#[test]
fn rename_keeps_export_alias_public_name() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(
        root.path().join("card.ts"),
        "function Card() { return 1; }\nexport { Card as PublicCard };\n",
    )
    .unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import { PublicCard } from \"./card\";\nexport const run = () => PublicCard();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let card = fs::read_to_string(root.path().join("card.ts")).unwrap();
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(card.contains("function ProfileCard()"), "{card}");
    assert!(card.contains("export { ProfileCard as PublicCard }"), "{card}");
    assert!(app.contains("PublicCard"), "{app}");
    assert!(!app.contains("ProfileCard"), "{app}");
}

#[test]
fn rename_keeps_default_export_alias_and_reports_importer_leftovers() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(
        root.path().join("card.ts"),
        "function Card() { return 1; }\nexport { Card as default };\n",
    )
    .unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import Card from \"./card\";\nexport const run = () => Card();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let card = fs::read_to_string(root.path().join("card.ts")).unwrap();
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(card.contains("function ProfileCard()"), "{card}");
    assert!(card.contains("export { ProfileCard as default }"), "{card}");
    assert!(app.contains("import Card from"), "{app}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"leftovers\":[{\"file\":\"app.ts\""), "{stdout}");
}

#[test]
fn rename_updates_namespace_import_member_access() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import * as cards from \"./card\";\nexport const run = () => cards.Card();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(app.contains("cards.ProfileCard()"), "{app}");
    assert!(!app.contains("cards.Card"), "{app}");
}

#[test]
fn rename_resolves_tsconfig_path_alias_imports() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("lib")).unwrap();
    fs::write(
        root.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@lib/*":["lib/*"]}},"include":["**/*.ts"]}"#,
    )
    .unwrap();
    fs::write(root.path().join("lib/card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import { Card } from \"@lib/card\";\nexport const run = () => Card();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args([
            "rename",
            "lib/card.ts#Card",
            "ProfileCard",
            "--root",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(app.contains("import { ProfileCard } from \"@lib/card\""), "{app}");
    assert!(app.contains("ProfileCard()"), "{app}");
}

#[test]
fn rename_interface_member_updates_typed_accesses() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("types.ts"), "export interface User {\n  title: string;\n}\n")
        .unwrap();
    fs::write(
        root.path().join("use.ts"),
        "import type { User } from \"./types\";\nexport const read = (u: User) => u.title;\nexport const make = (): User => ({ title: \"x\" });\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "types.ts#title", "headline", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let usage = fs::read_to_string(root.path().join("use.ts")).unwrap();
    assert!(usage.contains("u.headline"), "{usage}");
    assert!(usage.contains("headline: \"x\""), "{usage}");
    assert!(!usage.contains("title"), "{usage}");
}

#[test]
fn rename_type_alias_with_custom_tsconfig_path() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("config")).unwrap();
    fs::write(root.path().join("config/tsconfig.base.json"), r#"{"include":["../*.ts"]}"#).unwrap();
    fs::write(root.path().join("types.ts"), "export type Card = { id: string };\n").unwrap();
    fs::write(
        root.path().join("use.ts"),
        "import type { Card } from \"./types\";\nexport const pick = (c: Card) => c.id;\n",
    )
    .unwrap();
    let tsconfig = root.path().join("config/tsconfig.base.json");
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args([
            "rename",
            "Card",
            "ProfileCard",
            "--root",
            root.path().to_str().unwrap(),
            "--tsconfig",
            tsconfig.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let usage = fs::read_to_string(root.path().join("use.ts")).unwrap();
    assert!(usage.contains("ProfileCard"), "{usage}");
    assert!(!usage.contains("{ Card }"), "{usage}");
}

#[test]
fn rename_props_member_updates_jsx_attributes() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(
        root.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"jsx":"react-jsx"},"include":["*.tsx"]}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("card.tsx"),
        "interface CardProps {\n  title: string;\n}\nexport function Card(props: CardProps) { return <div>{props.title}</div>; }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("app.tsx"),
        "import { Card } from \"./card\";\nexport const App = () => <Card title=\"x\" />;\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.tsx#title", "headline", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let card = fs::read_to_string(root.path().join("card.tsx")).unwrap();
    let app = fs::read_to_string(root.path().join("app.tsx")).unwrap();
    assert!(card.contains("headline: string"), "{card}");
    assert!(card.contains("props.headline"), "{card}");
    assert!(app.contains("<Card headline=\"x\" />"), "{app}");
}

#[test]
fn rename_member_preserves_destructured_local_name() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("types.ts"), "export interface User {\n  title: string;\n}\n")
        .unwrap();
    fs::write(
        root.path().join("use.ts"),
        "import type { User } from \"./types\";\nexport const pick = (u: User) => {\n  const { title } = u;\n  return title;\n};\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "types.ts#title", "headline", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let usage = fs::read_to_string(root.path().join("use.ts")).unwrap();
    assert!(usage.contains("{ headline: title }"), "{usage}");
    assert!(usage.contains("return title;"), "{usage}");
}

#[test]
fn rename_enum_member_updates_dotted_access() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("status.ts"), "export enum Status {\n  Active = 1,\n}\n").unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import { Status } from \"./status\";\nexport const active = Status.Active;\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "status.ts#Active", "Enabled", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(app.contains("Status.Enabled"), "{app}");
    assert!(!app.contains("Status.Active"), "{app}");
}

#[test]
fn rename_leaves_shadowing_locals_untouched() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import { Card } from \"./card\";\nexport const a = Card();\nexport const shadow = () => {\n  const Card = 2;\n  return Card;\n};\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(app.contains("import { ProfileCard }"), "{app}");
    assert!(app.contains("export const a = ProfileCard();"), "{app}");
    assert!(app.contains("const Card = 2;"), "{app}");
    assert!(app.contains("return Card;"), "{app}");
}

#[test]
fn rename_member_updates_computed_string_access() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("types.ts"), "export interface User {\n  title: string;\n}\n")
        .unwrap();
    fs::write(
        root.path().join("use.ts"),
        "import type { User } from \"./types\";\nexport const read = (u: User) => u[\"title\"];\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "types.ts#title", "headline", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let usage = fs::read_to_string(root.path().join("use.ts")).unwrap();
    assert!(usage.contains("u[\"headline\"]"), "{usage}");
}

#[test]
fn rename_reports_importers_outside_tsconfig_as_leftovers() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("scripts")).unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["src/**/*.ts"]}"#).unwrap();
    fs::write(root.path().join("src/card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("scripts/tool.ts"),
        "import { Card } from \"../src/card\";\nexport const run = () => Card();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args([
            "rename",
            "src/card.ts#Card",
            "ProfileCard",
            "--root",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let tool = fs::read_to_string(root.path().join("scripts/tool.ts")).unwrap();
    assert!(tool.contains("Card"), "{tool}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("scripts/tool.ts"),
        "leftovers must flag out-of-project importer: {stdout}"
    );
}

#[test]
fn rename_covers_merged_interface_and_namespace() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(
        root.path().join("card.ts"),
        "export interface Card { id: string }\nexport namespace Card {\n  export const kind = \"k\";\n}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import { Card } from \"./card\";\nexport const c: Card = { id: \"1\" };\nexport const k = Card.kind;\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let card = fs::read_to_string(root.path().join("card.ts")).unwrap();
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(card.contains("interface ProfileCard"), "{card}");
    assert!(card.contains("namespace ProfileCard"), "{card}");
    assert!(app.contains("c: ProfileCard"), "{app}");
    assert!(app.contains("ProfileCard.kind"), "{app}");
}

#[test]
fn rename_leaves_jsdoc_link_but_reports_it_as_leftover() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import { Card } from \"./card\";\n/** Wraps {@link Card} with logging. */\nexport const run = () => Card();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(app.contains("ProfileCard()"), "{app}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    if app.contains("{@link Card}") {
        assert!(stdout.contains("{@link Card}"), "leftovers must flag stale JSDoc: {stdout}");
    } else {
        assert!(app.contains("{@link ProfileCard}"), "{app}");
    }
}

#[test]
fn rename_reports_commonjs_require_in_plain_js_as_leftover() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("tool.js"),
        "const { Card } = require(\"./card\");\nmodule.exports = () => Card();\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let tool = fs::read_to_string(root.path().join("tool.js")).unwrap();
    assert!(tool.contains("{ Card }"), "{tool}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tool.js"), "leftovers must flag the CJS consumer: {stdout}");
}

#[test]
fn rename_to_colliding_name_succeeds_and_tsc_is_the_verifier() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("app.ts"),
        "import { Card } from \"./card\";\nconst ProfileCard = 5;\nexport const both = () => Card() + ProfileCard;\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let app = fs::read_to_string(root.path().join("app.ts")).unwrap();
    assert!(app.contains("import { ProfileCard }"), "{app}");
    assert!(app.contains("const ProfileCard = 5;"), "{app}");
}

#[test]
fn rename_updates_dynamic_import_member_access() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("tsconfig.json"), r#"{"include":["*.ts"]}"#).unwrap();
    fs::write(root.path().join("card.ts"), "export function Card() { return 1; }\n").unwrap();
    fs::write(
        root.path().join("lazy.ts"),
        "export const load = () => import(\"./card\").then((m) => m.Card());\n",
    )
    .unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "card.ts#Card", "ProfileCard", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let lazy = fs::read_to_string(root.path().join("lazy.ts")).unwrap();
    assert!(lazy.contains("m.ProfileCard()"), "{lazy}");
    assert!(lazy.contains("import(\"./card\")"), "{lazy}");
}

#[test]
fn rename_unknown_symbol_errors_cleanly() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    write_fixture(root.path());
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args(["rename", "Nonexistent", "Whatever", "--root", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no declaration named Nonexistent"), "{stderr}");
}

#[test]
fn rename_dry_run_leaves_sources_unchanged() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    write_fixture(root.path());
    let card_before = fs::read(root.path().join("card.tsx")).unwrap();
    let app_before = fs::read(root.path().join("app.tsx")).unwrap();
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args([
            "rename",
            "Card",
            "ProfileCard",
            "--root",
            root.path().to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read(root.path().join("card.tsx")).unwrap(), card_before);
    assert_eq!(fs::read(root.path().join("app.tsx")).unwrap(), app_before);
    let card = fs::read_to_string(root.path().join("card.tsx")).unwrap();
    let app = fs::read_to_string(root.path().join("app.tsx")).unwrap();
    assert!(card.contains("Card"), "{card}");
    assert!(app.contains("Card"), "{app}");
}
