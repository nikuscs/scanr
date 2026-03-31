use std::path::Path;
use std::process::Command;

const CODE_EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"];
const DOC_EXTS: &[&str] = &["md", "mdx"];

pub fn list_files(root: &Path) -> anyhow::Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root)
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Not a git repository. Run from a git project root.");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files =
        stdout.lines().filter(|f| !f.is_empty() && is_indexable(f)).map(String::from).collect();

    Ok(files)
}

fn is_indexable(path: &str) -> bool {
    let Some(ext) = path.rsplit('.').next() else {
        return false;
    };
    CODE_EXTS.contains(&ext) || DOC_EXTS.contains(&ext)
}

pub fn lang_for_ext(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "md" | "mdx" => "markdown",
        _ => "",
    }
}

pub fn ext_for_path(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or("")
}
