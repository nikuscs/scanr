use std::path::Path;
use std::process::Command;

const CODE_EXTS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs", // JS/TS
    "rs",  // Rust
    "py",  // Python
    "go",  // Go
];
const DOC_EXTS: &[&str] = &["md", "mdx"];
const DATA_EXTS: &[&str] = &["json", "yaml", "yml", "toml"];

pub fn list_files(root: &Path, gitignore: Option<&str>) -> anyhow::Result<Vec<String>> {
    let mut cmd = Command::new("git");
    if let Some(path) = gitignore {
        cmd.arg("-c").arg(format!("core.excludesFile={path}"));
    }
    let output = cmd
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root)
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Not a git repository. Run from a git project root.");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> =
        stdout.lines().filter(|f| !f.is_empty() && is_indexable(f)).map(String::from).collect();

    Ok(files)
}

fn is_indexable(path: &str) -> bool {
    let Some(ext) = path.rsplit('.').next() else {
        return false;
    };
    CODE_EXTS.contains(&ext) || DOC_EXTS.contains(&ext) || DATA_EXTS.contains(&ext)
}

pub fn lang_for_ext(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "rs" => "rust",
        "py" => "python",
        "go" => "go",
        "md" | "mdx" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        _ => "",
    }
}

pub fn ext_for_path(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexable_code_files() {
        assert!(is_indexable("src/main.rs"));
        assert!(is_indexable("app.ts"));
        assert!(is_indexable("index.tsx"));
        assert!(is_indexable("lib.py"));
        assert!(is_indexable("main.go"));
        assert!(is_indexable("utils.mts"));
        assert!(is_indexable("config.cjs"));
    }

    #[test]
    fn indexable_doc_files() {
        assert!(is_indexable("README.md"));
        assert!(is_indexable("docs/guide.mdx"));
    }

    #[test]
    fn indexable_data_files() {
        assert!(is_indexable("config.json"));
        assert!(is_indexable("config.yaml"));
        assert!(is_indexable("config.yml"));
        assert!(is_indexable("Cargo.toml"));
    }

    #[test]
    fn non_indexable_files() {
        assert!(!is_indexable("image.png"));
        assert!(!is_indexable("binary.exe"));
        assert!(!is_indexable("styles.css"));
        assert!(!is_indexable("Makefile"));
        assert!(!is_indexable("noext"));
    }

    #[test]
    fn lang_for_ext_mapping() {
        assert_eq!(lang_for_ext("app.ts"), "typescript");
        assert_eq!(lang_for_ext("app.tsx"), "typescript");
        assert_eq!(lang_for_ext("app.mts"), "typescript");
        assert_eq!(lang_for_ext("app.js"), "javascript");
        assert_eq!(lang_for_ext("app.jsx"), "javascript");
        assert_eq!(lang_for_ext("main.rs"), "rust");
        assert_eq!(lang_for_ext("main.py"), "python");
        assert_eq!(lang_for_ext("main.go"), "go");
        assert_eq!(lang_for_ext("README.md"), "markdown");
        assert_eq!(lang_for_ext("config.json"), "json");
        assert_eq!(lang_for_ext("config.yaml"), "yaml");
        assert_eq!(lang_for_ext("config.yml"), "yaml");
        assert_eq!(lang_for_ext("config.toml"), "toml");
        assert_eq!(lang_for_ext("unknown.xyz"), "");
    }

    #[test]
    fn ext_for_path_extraction() {
        assert_eq!(ext_for_path("src/main.rs"), "rs");
        assert_eq!(ext_for_path("deep/nested/file.test.ts"), "ts");
        assert_eq!(ext_for_path("noext"), "noext");
    }

    // ---- is_indexable boundary cases ----

    #[test]
    fn is_indexable_bare_extension() {
        // A filename like ".rs" — rsplit('.') yields ["rs", ""] so ext = "rs"
        assert!(is_indexable(".rs"));
        assert!(is_indexable(".md"));
        assert!(is_indexable(".json"));
    }

    #[test]
    fn is_indexable_uppercase_not_matched() {
        // Extensions are case-sensitive; uppercase should not match
        assert!(!is_indexable("main.RS"));
        assert!(!is_indexable("app.TS"));
        assert!(!is_indexable("README.MD"));
        assert!(!is_indexable("config.JSON"));
        assert!(!is_indexable("config.YAML"));
        assert!(!is_indexable("Cargo.TOML"));
    }

    #[test]
    fn is_indexable_multiple_dots() {
        // Only the last extension matters
        assert!(is_indexable("foo.test.ts"));
        assert!(is_indexable("foo.spec.js"));
        assert!(is_indexable("some.thing.config.json"));
        assert!(is_indexable("deep/path/file.module.mts"));
        assert!(!is_indexable("foo.test.css"));
        assert!(!is_indexable("archive.tar.gz"));
    }

    #[test]
    fn is_indexable_empty_string() {
        assert!(!is_indexable(""));
    }

    #[test]
    fn is_indexable_dot_only() {
        // "." -> rsplit('.') yields ["", ""] so ext = ""
        assert!(!is_indexable("."));
    }

    #[test]
    fn is_indexable_hidden_dirs() {
        assert!(is_indexable(".config/settings.json"));
        assert!(is_indexable(".github/workflows/ci.yml"));
        assert!(!is_indexable(".git/HEAD"));
    }

    // ---- lang_for_ext edge cases ----

    #[test]
    fn lang_for_ext_cts() {
        assert_eq!(lang_for_ext("lib.cts"), "typescript");
    }

    #[test]
    fn lang_for_ext_cjs_mjs() {
        assert_eq!(lang_for_ext("lib.cjs"), "javascript");
        assert_eq!(lang_for_ext("lib.mjs"), "javascript");
    }

    #[test]
    fn lang_for_ext_mdx() {
        assert_eq!(lang_for_ext("docs/page.mdx"), "markdown");
    }

    #[test]
    fn lang_for_ext_no_extension() {
        assert_eq!(lang_for_ext("Makefile"), "");
        assert_eq!(lang_for_ext(""), "");
    }

    #[test]
    fn lang_for_ext_multiple_dots() {
        assert_eq!(lang_for_ext("foo.test.ts"), "typescript");
        assert_eq!(lang_for_ext("foo.spec.js"), "javascript");
        assert_eq!(lang_for_ext("a.b.c.json"), "json");
    }

    #[test]
    fn lang_for_ext_uppercase_returns_empty() {
        assert_eq!(lang_for_ext("main.RS"), "");
        assert_eq!(lang_for_ext("app.TS"), "");
    }

    // ---- ext_for_path edge cases ----

    #[test]
    fn ext_for_path_empty_string() {
        assert_eq!(ext_for_path(""), "");
    }

    #[test]
    fn ext_for_path_dot_only() {
        // "." -> rsplit('.') yields ["", ""] -> next() = ""
        assert_eq!(ext_for_path("."), "");
    }

    #[test]
    fn ext_for_path_hidden_file() {
        // ".gitignore" -> rsplit('.') yields ["gitignore", ""]
        assert_eq!(ext_for_path(".gitignore"), "gitignore");
    }

    #[test]
    fn ext_for_path_trailing_dot() {
        // "foo." -> rsplit('.') yields ["", "foo"]
        assert_eq!(ext_for_path("foo."), "");
    }

    #[test]
    fn ext_for_path_multiple_dots() {
        assert_eq!(ext_for_path("archive.tar.gz"), "gz");
        assert_eq!(ext_for_path("a.b.c.d"), "d");
    }

    // ---- list_files integration tests ----

    #[test]
    fn list_files_returns_tracked_indexable_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // git init
        let status = Command::new("git").args(["init"]).current_dir(root).output().unwrap();
        assert!(status.status.success(), "git init failed");

        // Configure required git identity for commits
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(root)
            .output()
            .unwrap();

        // Create indexable files
        std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("lib.ts"), "export {}").unwrap();
        std::fs::write(root.join("README.md"), "# Hello").unwrap();
        std::fs::write(root.join("config.json"), "{}").unwrap();

        // Create non-indexable files
        std::fs::write(root.join("image.png"), "fake png").unwrap();
        std::fs::write(root.join("styles.css"), "body {}").unwrap();

        // git add and commit everything
        Command::new("git").args(["add", "."]).current_dir(root).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(root).output().unwrap();

        let files = list_files(root, None).unwrap();

        assert!(files.contains(&"main.rs".to_string()));
        assert!(files.contains(&"lib.ts".to_string()));
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"config.json".to_string()));

        // Non-indexable files should be filtered out
        assert!(!files.contains(&"image.png".to_string()));
        assert!(!files.contains(&"styles.css".to_string()));
    }

    #[test]
    fn list_files_includes_untracked_indexable_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        Command::new("git").args(["init"]).current_dir(root).output().unwrap();

        // Create files but do NOT git add them — they are untracked
        // list_files uses --others so they should still appear
        std::fs::write(root.join("untracked.rs"), "fn foo() {}").unwrap();
        std::fs::write(root.join("also_untracked.py"), "pass").unwrap();

        let files = list_files(root, None).unwrap();

        assert!(files.contains(&"untracked.rs".to_string()));
        assert!(files.contains(&"also_untracked.py".to_string()));
    }

    #[test]
    fn list_files_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        Command::new("git").args(["init"]).current_dir(root).output().unwrap();

        // Create a .gitignore that ignores the build directory and a specific file
        std::fs::write(root.join(".gitignore"), "build/\nignored.rs\n").unwrap();

        // Create files
        std::fs::write(root.join("kept.rs"), "fn kept() {}").unwrap();
        std::fs::write(root.join("ignored.rs"), "fn ignored() {}").unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("build/output.js"), "var x;").unwrap();

        let files = list_files(root, None).unwrap();

        assert!(files.contains(&"kept.rs".to_string()));
        // gitignored files should not appear
        assert!(!files.contains(&"ignored.rs".to_string()));
        assert!(!files.contains(&"build/output.js".to_string()));
    }

    #[test]
    fn list_files_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        Command::new("git").args(["init"]).current_dir(root).output().unwrap();

        std::fs::create_dir_all(root.join("src/deep/nested")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("src/deep/nested/mod.rs"), "mod x;").unwrap();
        std::fs::write(root.join("src/deep/nested/config.toml"), "[pkg]").unwrap();

        Command::new("git").args(["add", "."]).current_dir(root).output().unwrap();

        let files = list_files(root, None).unwrap();

        assert!(files.contains(&"src/main.rs".to_string()));
        assert!(files.contains(&"src/deep/nested/mod.rs".to_string()));
        assert!(files.contains(&"src/deep/nested/config.toml".to_string()));
    }

    #[test]
    fn list_files_fails_on_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        // No git init — should fail
        let result = list_files(dir.path(), None);
        assert!(result.is_err());
    }
}
