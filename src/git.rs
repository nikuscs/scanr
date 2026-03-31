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
