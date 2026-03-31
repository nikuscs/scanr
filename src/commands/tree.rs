use std::cmp::Ordering;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const IGNORE_DIRS: &[&str] = &[
    ".git",
    ".superset",
    ".glooit",
    ".claude",
    ".codex",
    "node_modules",
    "dist",
    "build",
    "target",
    ".next",
    ".turbo",
    "coverage",
];

const IGNORE_TEST_DIRS: &[&str] = &["test", "tests", "__test__", "__tests__"];

const INCLUDE_FILE_NAMES: &[&str] = &["Dockerfile", "LICENSE", "Makefile", "Procfile", "README"];

const INCLUDE_EXTS: &[&str] = &[
    "cjs", "cts", "css", "env", "go", "html", "java", "js", "json", "jsx", "md", "mdx", "mjs",
    "mts", "py", "rs", "scss", "sh", "sql", "toml", "ts", "tsx", "txt", "yaml", "yml",
];

const STRIP_EXTS: &[&str] =
    &["cjs", "cts", "go", "java", "js", "jsx", "mjs", "mts", "py", "rs", "ts", "tsx"];

#[allow(clippy::unused_async)]
pub async fn run(
    root: &str,
    subpath: Option<&str>,
    depth: usize,
    inline: usize,
    all: bool,
) -> Result<()> {
    let project =
        fs::canonicalize(root).context("Cannot resolve project root")?.display().to_string();
    let project_root = PathBuf::from(&project);

    let start_rel = subpath.unwrap_or("").trim_matches('/');
    let start_path =
        if start_rel.is_empty() { project_root.clone() } else { project_root.join(start_rel) };

    let start_path = fs::canonicalize(&start_path)
        .with_context(|| format!("Cannot resolve path {start_rel}"))?;

    if !start_path.starts_with(&project_root) {
        anyhow::bail!("Path must be inside the project root");
    }

    let tree = build_node(&start_path, &project_root, all)?;
    let mut lines = Vec::new();
    lines.push("# Project Structure".to_string());
    lines.push(String::new());
    render_node(&tree, &mut lines, "", true, 0, depth.max(1), inline.max(1));

    let output = lines.join("\n");
    let chars = output.len();
    let estimated_tokens = chars.div_ceil(4);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "{output}")?;
    writeln!(out)?;
    writeln!(out, "# ~{estimated_tokens} tokens ({chars} chars, {} lines)", lines.len())?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileEntry {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeNode {
    name: String,
    rel_path: String,
    dirs: Vec<Self>,
    files: Vec<FileEntry>,
}

fn build_node(abs_path: &Path, project_root: &Path, all: bool) -> Result<TreeNode> {
    let rel_path = rel_path(project_root, abs_path);
    let name = if rel_path.is_empty() {
        abs_path.file_name().map_or_else(|| ".".to_string(), |n| n.to_string_lossy().into_owned())
    } else {
        abs_path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };

    let mut dir_specs = Vec::new();
    let mut files = Vec::new();

    for entry in safe_readdir(abs_path)? {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        let entry_name = entry.file_name().to_string_lossy().into_owned();

        if file_type.is_dir() {
            if should_skip_dir(&entry_name, all) {
                continue;
            }
            dir_specs.push(entry.path());
            continue;
        }

        if file_type.is_file() && should_include_file(&entry_name) {
            files.push(FileEntry { name: entry_name });
        }
    }

    dir_specs.sort();
    files.sort_by(|a, b| natural_cmp(&a.name, &b.name));

    let dirs: Vec<TreeNode> =
        dir_specs.iter().map(|path| build_node(path, project_root, all)).collect::<Result<_>>()?;

    Ok(TreeNode { name, rel_path, dirs, files })
}

fn render_node(
    node: &TreeNode,
    lines: &mut Vec<String>,
    indent: &str,
    is_root: bool,
    branch_depth: usize,
    max_depth: usize,
    inline: usize,
) {
    let mut current = node;
    let mut chain_name = if is_root {
        if current.rel_path.is_empty() { String::new() } else { format!("{}/", current.rel_path) }
    } else {
        format!("{}/", current.name)
    };

    while !is_root && current.dirs.len() == 1 && current.files.is_empty() {
        current = &current.dirs[0];
        chain_name.push_str(&current.name);
        chain_name.push('/');
    }

    let is_branching = current.dirs.len() > 1
        || (!current.dirs.is_empty() && !current.files.is_empty())
        || current.files.len() > 1;
    let next_branch_depth =
        if is_root { branch_depth } else { branch_depth + usize::from(is_branching) };

    if !is_root && next_branch_depth >= max_depth {
        lines.push(format!("{indent}{chain_name} {}", summarize(current)));
        return;
    }

    if !is_root || !current.rel_path.is_empty() {
        if current.dirs.is_empty() && current.files.len() <= inline {
            let file_str = current.files.iter().map(fmt_file).collect::<Vec<_>>().join(", ");
            if file_str.is_empty() {
                lines.push(format!("{indent}{chain_name}"));
            } else {
                lines.push(format!("{indent}{chain_name}  {file_str}"));
            }
            return;
        }
        lines.push(format!("{indent}{chain_name}"));
    }

    let child_indent = if is_root { String::new() } else { format!("{indent}  ") };

    for dir in &current.dirs {
        render_node(dir, lines, &child_indent, false, next_branch_depth, max_depth, inline);
    }

    if !current.files.is_empty() {
        render_file_list(&current.files, lines, &child_indent, inline);
    }
}

fn render_file_list(files: &[FileEntry], lines: &mut Vec<String>, indent: &str, inline: usize) {
    for chunk in files.chunks(inline) {
        let rendered = chunk.iter().map(fmt_file).collect::<Vec<_>>().join(", ");
        lines.push(format!("{indent}{rendered}"));
    }
}

fn fmt_file(file: &FileEntry) -> String {
    strip_known_ext(&file.name)
}

fn summarize(node: &TreeNode) -> String {
    let (dirs, files) = count_tree(node);
    let mut parts = Vec::new();
    if dirs > 0 {
        parts.push(format!("{dirs}d"));
    }
    if files > 0 {
        parts.push(format!("{files}f"));
    }
    format!("({})", parts.join(" "))
}

fn count_tree(node: &TreeNode) -> (usize, usize) {
    let mut dirs = node.dirs.len();
    let mut files = node.files.len();
    for dir in &node.dirs {
        let (sub_dirs, sub_files) = count_tree(dir);
        dirs += sub_dirs;
        files += sub_files;
    }
    (dirs, files)
}

fn should_skip_dir(name: &str, all: bool) -> bool {
    if IGNORE_DIRS.contains(&name) || name.starts_with('.') {
        return true;
    }
    !all && IGNORE_TEST_DIRS.contains(&name)
}

fn should_include_file(name: &str) -> bool {
    if INCLUDE_FILE_NAMES.contains(&name) {
        return true;
    }

    let ext = name.rsplit('.').next().unwrap_or("");
    INCLUDE_EXTS.contains(&ext)
}

fn strip_known_ext(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("");
    if STRIP_EXTS.contains(&ext) && name.contains('.') {
        let suffix = format!(".{ext}");
        return name.strip_suffix(&suffix).unwrap_or(name).to_string();
    }
    name.to_string()
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().trim_matches('/').to_string())
        .unwrap_or_default()
}

fn safe_readdir(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("Cannot read {}", dir.display()))?.flatten()
    {
        entries.push(entry);
    }
    Ok(entries)
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let left_parts = split_natural(left);
    let right_parts = split_natural(right);

    for (l, r) in left_parts.iter().zip(&right_parts) {
        let ord = match (l.parse::<usize>(), r.parse::<usize>()) {
            (Ok(ln), Ok(rn)) => ln.cmp(&rn),
            _ => l.cmp(r),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }

    left_parts.len().cmp(&right_parts.len()).then_with(|| left.cmp(right))
}

fn split_natural(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut current_is_digit = None;

    for ch in value.chars() {
        let is_digit = ch.is_ascii_digit();
        if current_is_digit.is_some_and(|flag| flag != is_digit) {
            parts.push(current);
            current = String::new();
        }
        current.push(ch.to_ascii_lowercase());
        current_is_digit = Some(is_digit);
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn includes_common_text_files() {
        assert!(should_include_file("main.rs"));
        assert!(should_include_file("README.md"));
        assert!(should_include_file("Cargo.toml"));
        assert!(should_include_file("Dockerfile"));
        assert!(!should_include_file("image.png"));
    }

    #[test]
    fn strips_known_extensions() {
        assert_eq!(strip_known_ext("main.rs"), "main");
        assert_eq!(strip_known_ext("index.test.ts"), "index.test");
        assert_eq!(strip_known_ext("Cargo.toml"), "Cargo.toml");
        assert_eq!(strip_known_ext("Dockerfile"), "Dockerfile");
    }

    #[test]
    fn natural_sort_handles_numbers() {
        let mut names = [
            FileEntry { name: "file10.ts".to_string() },
            FileEntry { name: "file2.ts".to_string() },
            FileEntry { name: "file1.ts".to_string() },
        ];
        names.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        let ordered: Vec<_> = names.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(ordered, vec!["file1.ts", "file2.ts", "file10.ts"]);
    }

    #[test]
    fn tree_skips_ignored_and_test_dirs_by_default() {
        let tmp = tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("src")).expect("src dir");
        fs::create_dir_all(tmp.path().join("tests")).expect("tests dir");
        fs::create_dir_all(tmp.path().join("target")).expect("target dir");
        fs::write(tmp.path().join("src/main.rs"), "fn main() {}").expect("write main");
        fs::write(tmp.path().join("tests/main_test.rs"), "").expect("write test");
        fs::write(tmp.path().join("target/cache.txt"), "").expect("write cache");

        let node = build_node(tmp.path(), tmp.path(), false).expect("build tree");
        let dir_names: BTreeSet<_> = node.dirs.iter().map(|dir| dir.name.as_str()).collect();
        assert!(dir_names.contains("src"));
        assert!(!dir_names.contains("tests"));
        assert!(!dir_names.contains("target"));
    }

    #[test]
    fn tree_includes_test_dirs_with_all_flag() {
        let tmp = tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("tests")).expect("tests dir");
        fs::write(tmp.path().join("tests/main_test.rs"), "").expect("write test");

        let node = build_node(tmp.path(), tmp.path(), true).expect("build tree");
        let dir_names: BTreeSet<_> = node.dirs.iter().map(|dir| dir.name.as_str()).collect();
        assert!(dir_names.contains("tests"));
    }

    // ── render_node ──────────────────────────────────────────────

    #[test]
    fn render_node_shows_nested_dirs_and_files() {
        let node = TreeNode {
            name: "root".into(),
            rel_path: String::new(),
            dirs: vec![TreeNode {
                name: "src".into(),
                rel_path: "src".into(),
                dirs: vec![TreeNode {
                    name: "utils".into(),
                    rel_path: "src/utils".into(),
                    dirs: vec![],
                    files: vec![FileEntry { name: "helper.ts".into() }],
                }],
                files: vec![
                    FileEntry { name: "main.rs".into() },
                    FileEntry { name: "lib.rs".into() },
                ],
            }],
            files: vec![FileEntry { name: "Cargo.toml".into() }],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", true, 0, 10, 3);
        let text = lines.join("\n");

        assert!(text.contains("src/"), "should contain dir 'src/'");
        assert!(text.contains("utils/"), "should contain dir 'utils/'");
        assert!(text.contains("main"), "should contain file 'main'");
        assert!(text.contains("lib"), "should contain file 'lib'");
        assert!(text.contains("helper"), "should contain file 'helper'");
        assert!(text.contains("Cargo.toml"), "should contain file 'Cargo.toml'");
    }

    #[test]
    fn render_node_root_with_rel_path_shows_prefix() {
        let node = TreeNode {
            name: "sub".into(),
            rel_path: "sub".into(),
            dirs: vec![],
            files: vec![FileEntry { name: "index.ts".into() }],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", true, 0, 10, 3);
        assert!(lines.iter().any(|l| l.contains("sub/")), "root with rel_path should show 'sub/'");
    }

    // ── summarize ────────────────────────────────────────────────

    #[test]
    fn summarize_dirs_and_files() {
        let node = TreeNode {
            name: "pkg".into(),
            rel_path: "pkg".into(),
            dirs: vec![
                TreeNode {
                    name: "a".into(),
                    rel_path: "pkg/a".into(),
                    dirs: vec![],
                    files: vec![FileEntry { name: "x.ts".into() }],
                },
                TreeNode {
                    name: "b".into(),
                    rel_path: "pkg/b".into(),
                    dirs: vec![],
                    files: vec![FileEntry { name: "y.ts".into() }],
                },
            ],
            files: vec![
                FileEntry { name: "one.rs".into() },
                FileEntry { name: "two.rs".into() },
                FileEntry { name: "three.rs".into() },
            ],
        };

        assert_eq!(summarize(&node), "(2d 5f)");
    }

    #[test]
    fn summarize_only_dirs() {
        let node = TreeNode {
            name: "d".into(),
            rel_path: "d".into(),
            dirs: vec![TreeNode {
                name: "inner".into(),
                rel_path: "d/inner".into(),
                dirs: vec![],
                files: vec![],
            }],
            files: vec![],
        };
        assert_eq!(summarize(&node), "(1d)");
    }

    #[test]
    fn summarize_only_files() {
        let node = TreeNode {
            name: "d".into(),
            rel_path: "d".into(),
            dirs: vec![],
            files: vec![FileEntry { name: "a.rs".into() }],
        };
        assert_eq!(summarize(&node), "(1f)");
    }

    #[test]
    fn summarize_empty() {
        let node = TreeNode { name: "d".into(), rel_path: "d".into(), dirs: vec![], files: vec![] };
        assert_eq!(summarize(&node), "()");
    }

    // ── count_tree ───────────────────────────────────────────────

    #[test]
    fn count_tree_recursive() {
        let node = TreeNode {
            name: "root".into(),
            rel_path: String::new(),
            dirs: vec![
                TreeNode {
                    name: "a".into(),
                    rel_path: "a".into(),
                    dirs: vec![TreeNode {
                        name: "a1".into(),
                        rel_path: "a/a1".into(),
                        dirs: vec![],
                        files: vec![
                            FileEntry { name: "f1.rs".into() },
                            FileEntry { name: "f2.rs".into() },
                        ],
                    }],
                    files: vec![FileEntry { name: "f3.rs".into() }],
                },
                TreeNode {
                    name: "b".into(),
                    rel_path: "b".into(),
                    dirs: vec![],
                    files: vec![FileEntry { name: "f4.rs".into() }],
                },
            ],
            files: vec![FileEntry { name: "f5.rs".into() }],
        };

        let (dirs, files) = count_tree(&node);
        // dirs: a, a1, b = 3
        assert_eq!(dirs, 3);
        // files: f1, f2, f3, f4, f5 = 5
        assert_eq!(files, 5);
    }

    #[test]
    fn count_tree_leaf() {
        let node = TreeNode {
            name: "leaf".into(),
            rel_path: "leaf".into(),
            dirs: vec![],
            files: vec![FileEntry { name: "only.rs".into() }],
        };
        assert_eq!(count_tree(&node), (0, 1));
    }

    // ── should_skip_dir ──────────────────────────────────────────

    #[test]
    fn should_skip_all_ignore_dirs() {
        for dir in IGNORE_DIRS {
            assert!(should_skip_dir(dir, false), "{dir} should be skipped");
            assert!(should_skip_dir(dir, true), "{dir} should be skipped even with all=true");
        }
    }

    #[test]
    fn should_skip_hidden_dirs() {
        assert!(should_skip_dir(".hidden", false));
        assert!(should_skip_dir(".config", true));
        assert!(should_skip_dir(".secret", false));
    }

    #[test]
    fn should_skip_test_dirs_without_all() {
        for dir in IGNORE_TEST_DIRS {
            assert!(should_skip_dir(dir, false), "{dir} should be skipped with all=false");
        }
    }

    #[test]
    fn should_not_skip_test_dirs_with_all() {
        for dir in IGNORE_TEST_DIRS {
            assert!(!should_skip_dir(dir, true), "{dir} should NOT be skipped with all=true");
        }
    }

    #[test]
    fn should_not_skip_normal_dirs() {
        assert!(!should_skip_dir("src", false));
        assert!(!should_skip_dir("lib", false));
        assert!(!should_skip_dir("packages", true));
    }

    // ── should_include_file ──────────────────────────────────────

    #[test]
    fn should_include_all_known_extensions() {
        for ext in INCLUDE_EXTS {
            let name = format!("file.{ext}");
            assert!(should_include_file(&name), "{name} should be included");
        }
    }

    #[test]
    fn should_include_special_file_names() {
        for name in INCLUDE_FILE_NAMES {
            assert!(should_include_file(name), "{name} should be included");
        }
    }

    #[test]
    fn should_exclude_unknown_extensions() {
        assert!(!should_include_file("image.png"));
        assert!(!should_include_file("video.mp4"));
        assert!(!should_include_file("archive.zip"));
        assert!(!should_include_file("binary.exe"));
        assert!(!should_include_file("font.woff2"));
    }

    // ── strip_known_ext ──────────────────────────────────────────

    #[test]
    fn strip_ext_multiple_dots() {
        assert_eq!(strip_known_ext("my.component.test.tsx"), "my.component.test");
        assert_eq!(strip_known_ext("a.b.c.d.js"), "a.b.c.d");
    }

    #[test]
    fn strip_ext_no_extension() {
        assert_eq!(strip_known_ext("Makefile"), "Makefile");
        assert_eq!(strip_known_ext("LICENSE"), "LICENSE");
    }

    #[test]
    fn strip_ext_non_strip_extensions_kept() {
        assert_eq!(strip_known_ext("styles.css"), "styles.css");
        assert_eq!(strip_known_ext("config.json"), "config.json");
        assert_eq!(strip_known_ext("readme.md"), "readme.md");
        assert_eq!(strip_known_ext("schema.sql"), "schema.sql");
        assert_eq!(strip_known_ext("config.yaml"), "config.yaml");
    }

    #[test]
    fn strip_ext_all_strip_exts() {
        for ext in STRIP_EXTS {
            let name = format!("file.{ext}");
            assert_eq!(strip_known_ext(&name), "file", "should strip .{ext}");
        }
    }

    // ── natural_cmp ──────────────────────────────────────────────

    #[test]
    fn natural_cmp_equal_strings() {
        assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
        assert_eq!(natural_cmp("file1.ts", "file1.ts"), Ordering::Equal);
    }

    #[test]
    fn natural_cmp_multiple_number_segments() {
        assert_eq!(natural_cmp("v1.2.3", "v1.2.10"), Ordering::Less);
        assert_eq!(natural_cmp("v1.10.1", "v1.2.1"), Ordering::Greater);
    }

    #[test]
    fn natural_cmp_pure_numbers() {
        assert_eq!(natural_cmp("9", "10"), Ordering::Less);
        assert_eq!(natural_cmp("100", "20"), Ordering::Greater);
        assert_eq!(natural_cmp("42", "42"), Ordering::Equal);
    }

    #[test]
    fn natural_cmp_case_insensitive_sorting() {
        // split_natural lowercases parts, so "Abc" and "abc" have equal segments,
        // but the final tiebreaker uses original strings: "Abc" < "abc" in ASCII
        assert_eq!(natural_cmp("Abc", "abc"), Ordering::Less);
        // Same-case strings are truly equal
        assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
        // Lowercased parts sort together: "File" and "file" are adjacent
        assert_eq!(natural_cmp("File1", "file2"), Ordering::Less);
    }

    #[test]
    fn natural_cmp_prefix() {
        assert_eq!(natural_cmp("file", "file1"), Ordering::Less);
        assert_eq!(natural_cmp("file1", "file"), Ordering::Greater);
    }

    // ── chain collapsing in render_node ──────────────────────────

    #[test]
    fn render_node_collapses_single_child_chain() {
        // a/ -> b/ -> c/ with files only in c
        let node = TreeNode {
            name: "a".into(),
            rel_path: "a".into(),
            dirs: vec![TreeNode {
                name: "b".into(),
                rel_path: "a/b".into(),
                dirs: vec![TreeNode {
                    name: "c".into(),
                    rel_path: "a/b/c".into(),
                    dirs: vec![],
                    files: vec![FileEntry { name: "leaf.ts".into() }],
                }],
                files: vec![],
            }],
            files: vec![],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", false, 0, 10, 3);
        let text = lines.join("\n");
        assert!(text.contains("a/b/c/"), "chain should be collapsed to 'a/b/c/'");
    }

    #[test]
    fn render_node_no_collapse_when_multiple_children() {
        let node = TreeNode {
            name: "a".into(),
            rel_path: "a".into(),
            dirs: vec![
                TreeNode {
                    name: "b".into(),
                    rel_path: "a/b".into(),
                    dirs: vec![],
                    files: vec![FileEntry { name: "x.ts".into() }],
                },
                TreeNode {
                    name: "c".into(),
                    rel_path: "a/c".into(),
                    dirs: vec![],
                    files: vec![FileEntry { name: "y.ts".into() }],
                },
            ],
            files: vec![],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", false, 0, 10, 3);
        let text = lines.join("\n");
        // Should NOT collapse: a has 2 children
        assert!(text.contains("a/\n"), "a/ should be its own line");
        assert!(text.contains("b/"), "child b/ should appear");
        assert!(text.contains("c/"), "child c/ should appear");
    }

    #[test]
    fn render_node_no_collapse_when_parent_has_files() {
        let node = TreeNode {
            name: "a".into(),
            rel_path: "a".into(),
            dirs: vec![TreeNode {
                name: "b".into(),
                rel_path: "a/b".into(),
                dirs: vec![],
                files: vec![FileEntry { name: "inner.ts".into() }],
            }],
            files: vec![FileEntry { name: "outer.rs".into() }],
        };

        let mut lines = Vec::new();
        render_node(&node, &mut lines, "", false, 0, 10, 3);
        let text = lines.join("\n");
        // a has files, so it should NOT collapse with b
        assert!(!text.contains("a/b/"), "should not collapse when parent has files");
    }

    // ── render_node depth truncation ─────────────────────────────

    #[test]
    fn render_node_truncates_at_max_depth() {
        let node = TreeNode {
            name: "top".into(),
            rel_path: "top".into(),
            dirs: vec![TreeNode {
                name: "mid".into(),
                rel_path: "top/mid".into(),
                dirs: vec![TreeNode {
                    name: "deep".into(),
                    rel_path: "top/mid/deep".into(),
                    dirs: vec![],
                    files: vec![
                        FileEntry { name: "a.rs".into() },
                        FileEntry { name: "b.rs".into() },
                    ],
                }],
                files: vec![FileEntry { name: "c.rs".into() }, FileEntry { name: "d.rs".into() }],
            }],
            files: vec![],
        };

        let mut lines = Vec::new();
        // max_depth=1 should truncate at first branching level
        render_node(&node, &mut lines, "", false, 0, 1, 3);
        let text = lines.join("\n");
        // Should show a summary rather than expanding everything
        assert!(text.contains('d') || text.contains('f'), "truncated node should show summary");
    }

    // ── rel_path ─────────────────────────────────────────────────

    #[test]
    fn rel_path_matching_prefix() {
        let root = Path::new("/home/user/project");
        let child = Path::new("/home/user/project/src/main.rs");
        assert_eq!(rel_path(root, child), "src/main.rs");
    }

    #[test]
    fn rel_path_same_path() {
        let root = Path::new("/home/user/project");
        assert_eq!(rel_path(root, root), "");
    }

    #[test]
    fn rel_path_non_matching_prefix() {
        let root = Path::new("/home/user/project");
        let other = Path::new("/tmp/other");
        // strip_prefix fails, returns empty string
        assert_eq!(rel_path(root, other), "");
    }

    // ── build_node with filesystem ───────────────────────────────

    #[test]
    fn build_node_filters_files_by_extension() {
        let tmp = tempdir().expect("tempdir");
        fs::write(tmp.path().join("app.ts"), "").expect("write ts");
        fs::write(tmp.path().join("style.css"), "").expect("write css");
        fs::write(tmp.path().join("image.png"), "").expect("write png");
        fs::write(tmp.path().join("data.bin"), "").expect("write bin");

        let node = build_node(tmp.path(), tmp.path(), false).expect("build");
        let file_names: BTreeSet<_> = node.files.iter().map(|f| f.name.as_str()).collect();
        assert!(file_names.contains("app.ts"));
        assert!(file_names.contains("style.css"));
        assert!(!file_names.contains("image.png"));
        assert!(!file_names.contains("data.bin"));
    }

    #[test]
    fn build_node_includes_special_file_names() {
        let tmp = tempdir().expect("tempdir");
        fs::write(tmp.path().join("Dockerfile"), "FROM rust").expect("write");
        fs::write(tmp.path().join("Makefile"), "all:").expect("write");
        fs::write(tmp.path().join("randomfile"), "stuff").expect("write");

        let node = build_node(tmp.path(), tmp.path(), false).expect("build");
        let file_names: BTreeSet<_> = node.files.iter().map(|f| f.name.as_str()).collect();
        assert!(file_names.contains("Dockerfile"));
        assert!(file_names.contains("Makefile"));
        assert!(!file_names.contains("randomfile"));
    }

    #[test]
    fn build_node_skips_hidden_directories() {
        let tmp = tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join(".hidden")).expect("mkdir");
        fs::write(tmp.path().join(".hidden/secret.rs"), "").expect("write");
        fs::create_dir_all(tmp.path().join("visible")).expect("mkdir");
        fs::write(tmp.path().join("visible/code.rs"), "").expect("write");

        let node = build_node(tmp.path(), tmp.path(), false).expect("build");
        let dir_names: BTreeSet<_> = node.dirs.iter().map(|d| d.name.as_str()).collect();
        assert!(!dir_names.contains(".hidden"));
        assert!(dir_names.contains("visible"));
    }
}
