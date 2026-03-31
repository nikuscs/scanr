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
}
