pub mod output;
pub mod types;
pub mod typescript;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use rayon::prelude::*;

use types::{FileIndex, FunctionKindsFilter, ScanResult, Stats};

const DEFAULT_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx"];
const DEFAULT_EXCLUDE_DIRS: &[&str] =
    &["node_modules", "dist", "build", ".next", ".git", "coverage", ".turbo", ".cache"];
const DEFAULT_MAX_BYTES: u64 = 1_048_576; // 1 MB

pub struct ScanConfig {
    pub extensions: Vec<String>,
    pub exclude: Vec<String>,
    pub max_bytes: u64,
    pub function_kinds: FunctionKindsFilter,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            extensions: vec!["ts".into(), "tsx".into(), "js".into(), "jsx".into()],
            exclude: Vec::new(),
            max_bytes: DEFAULT_MAX_BYTES,
            function_kinds: FunctionKindsFilter::All,
        }
    }
}

/// Scan a directory for JS/TS functions, bindings, and exports.
pub fn scan_directory(root: &Path, config: &ScanConfig) -> Result<ScanResult> {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let files = collect_files(&canonical_root, config)?;

    let total_files = files.len();
    let filter = config.function_kinds;

    let results: Vec<Result<FileIndex>> = files
        .par_iter()
        .map(|path| typescript::parse::process_file(path, &canonical_root, filter))
        .collect();

    let mut file_indices = Vec::with_capacity(results.len());
    let mut errors = Vec::new();
    let mut parsed = 0usize;
    let mut error_count = 0usize;

    for result in results {
        match result {
            Ok(fi) => {
                if fi.parse_errors > 0 {
                    error_count += 1;
                }
                parsed += 1;
                file_indices.push(fi);
            }
            Err(e) => {
                error_count += 1;
                errors.push(format!("{e:#}"));
            }
        }
    }

    file_indices.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(ScanResult {
        ver: 1,
        root: canonical_root.to_string_lossy().to_string(),
        stats: Stats {
            files: total_files,
            parsed,
            skipped: total_files.saturating_sub(parsed),
            errors: error_count,
        },
        file_indices,
        errors,
    })
}

/// Scan a single file.
pub fn scan_file(path: &Path, root: &Path, filter: FunctionKindsFilter) -> Result<FileIndex> {
    typescript::parse::process_file(path, root, filter)
}

fn collect_files(root: &Path, config: &ScanConfig) -> Result<Vec<PathBuf>> {
    let extensions: Vec<&str> = if config.extensions.is_empty() {
        DEFAULT_EXTENSIONS.to_vec()
    } else {
        config.extensions.iter().map(String::as_str).collect()
    };

    let mut builder = WalkBuilder::new(root);
    builder.hidden(true).git_ignore(true).git_global(true);

    let mut overrides = ignore::overrides::OverrideBuilder::new(root);
    for pattern in DEFAULT_EXCLUDE_DIRS {
        let pat = pattern.trim_matches('/');
        overrides
            .add(&format!("!**/{pat}/**"))
            .with_context(|| format!("invalid default exclude pattern: {pattern}"))?;
    }
    for pattern in &config.exclude {
        let pat = pattern.trim_matches('/');
        overrides
            .add(&format!("!**/{pat}/**"))
            .with_context(|| format!("invalid exclude pattern: {pattern}"))?;
    }
    let overrides = overrides.build().context("failed to build exclude overrides")?;
    builder.overrides(overrides);

    let mut files = Vec::new();

    for entry in builder.build() {
        let entry = entry.context("walk error")?;

        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let path = entry.path();

        if !has_matching_extension(path, &extensions) {
            continue;
        }

        if let Ok(meta) = entry.metadata()
            && meta.len() > config.max_bytes
        {
            tracing::debug!(path = %path.display(), "skipping oversized file");
            continue;
        }

        files.push(path.to_path_buf());
    }

    files.sort();
    Ok(files)
}

fn has_matching_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.iter().any(|&e| e.eq_ignore_ascii_case(ext)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ── has_matching_extension ────────────────────────────────────────

    #[test]
    fn has_matching_extension_matches_ts() {
        let path = Path::new("foo/bar.ts");
        assert!(has_matching_extension(path, &["ts", "tsx"]));
    }

    #[test]
    fn has_matching_extension_matches_tsx() {
        let path = Path::new("component.tsx");
        assert!(has_matching_extension(path, &["ts", "tsx"]));
    }

    #[test]
    fn has_matching_extension_no_match() {
        let path = Path::new("image.png");
        assert!(!has_matching_extension(path, &["ts", "tsx", "js", "jsx"]));
    }

    #[test]
    fn has_matching_extension_case_insensitive() {
        let path = Path::new("file.TS");
        assert!(has_matching_extension(path, &["ts"]));
    }

    #[test]
    fn has_matching_extension_no_extension() {
        let path = Path::new("Makefile");
        assert!(!has_matching_extension(path, &["ts", "js"]));
    }

    #[test]
    fn has_matching_extension_dot_file() {
        let path = Path::new(".gitignore");
        assert!(!has_matching_extension(path, &["ts", "js"]));
    }

    #[test]
    fn has_matching_extension_empty_extensions_list() {
        let path = Path::new("file.ts");
        assert!(!has_matching_extension(path, &[]));
    }

    // ── ScanConfig::default ──────────────────────────────────────────

    #[test]
    fn scan_config_default_extensions() {
        let cfg = ScanConfig::default();
        assert_eq!(cfg.extensions, vec!["ts", "tsx", "js", "jsx"]);
    }

    #[test]
    fn scan_config_default_exclude_is_empty() {
        let cfg = ScanConfig::default();
        assert!(cfg.exclude.is_empty());
    }

    #[test]
    fn scan_config_default_max_bytes() {
        let cfg = ScanConfig::default();
        assert_eq!(cfg.max_bytes, 1_048_576);
    }

    #[test]
    fn scan_config_default_function_kinds() {
        let cfg = ScanConfig::default();
        assert_eq!(cfg.function_kinds, FunctionKindsFilter::All);
    }

    // ── collect_files ────────────────────────────────────────────────

    #[test]
    fn collect_files_returns_only_matching_extensions() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("app.ts"), "const x = 1;").unwrap();
        fs::write(dir.path().join("util.js"), "const y = 2;").unwrap();
        fs::write(dir.path().join("image.png"), [0u8; 8]).unwrap();
        fs::write(dir.path().join("readme.txt"), "hello").unwrap();

        let config = ScanConfig::default();
        let files = collect_files(dir.path(), &config).unwrap();

        let names: Vec<String> =
            files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();

        assert!(names.contains(&"app.ts".to_string()));
        assert!(names.contains(&"util.js".to_string()));
        assert!(!names.contains(&"image.png".to_string()));
        assert!(!names.contains(&"readme.txt".to_string()));
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn collect_files_respects_exclude_patterns() {
        let dir = tempdir().unwrap();
        let custom_dir = dir.path().join("custom_vendor");
        fs::create_dir_all(&custom_dir).unwrap();
        fs::write(custom_dir.join("lib.ts"), "export {};").unwrap();
        fs::write(dir.path().join("main.ts"), "const z = 3;").unwrap();

        let config = ScanConfig { exclude: vec!["custom_vendor".into()], ..ScanConfig::default() };
        let files = collect_files(dir.path(), &config).unwrap();

        let names: Vec<String> =
            files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();

        assert!(names.contains(&"main.ts".to_string()));
        assert!(!names.contains(&"lib.ts".to_string()));
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn collect_files_skips_oversized_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("small.ts"), "const a = 1;").unwrap();
        // Create a file that exceeds the max_bytes threshold
        let big_content = "x".repeat(256);
        fs::write(dir.path().join("big.ts"), &big_content).unwrap();

        let config = ScanConfig { max_bytes: 128, ..ScanConfig::default() };
        let files = collect_files(dir.path(), &config).unwrap();

        let names: Vec<String> =
            files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();

        assert!(names.contains(&"small.ts".to_string()));
        assert!(!names.contains(&"big.ts".to_string()));
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn collect_files_excludes_default_dirs() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("dep.ts"), "export {};").unwrap();
        fs::write(dir.path().join("index.ts"), "import {};").unwrap();

        let config = ScanConfig::default();
        let files = collect_files(dir.path(), &config).unwrap();

        let names: Vec<String> =
            files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();

        assert!(names.contains(&"index.ts".to_string()));
        assert!(!names.contains(&"dep.ts".to_string()));
    }

    #[test]
    fn collect_files_empty_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let config = ScanConfig::default();
        let files = collect_files(dir.path(), &config).unwrap();
        assert!(files.is_empty());
    }

    // ── scan_directory ───────────────────────────────────────────────

    #[test]
    fn scan_directory_produces_correct_stats() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.ts"), "export function hello() { return 1; }").unwrap();
        fs::write(dir.path().join("b.ts"), "export const greet = () => 'hi';").unwrap();

        let config = ScanConfig::default();
        let result = scan_directory(dir.path(), &config).unwrap();

        assert_eq!(result.ver, 1);
        assert_eq!(result.stats.files, 2);
        assert_eq!(result.stats.parsed, 2);
        assert_eq!(result.stats.skipped, 0);
        assert_eq!(result.stats.errors, 0);
        assert_eq!(result.file_indices.len(), 2);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn scan_directory_file_indices_are_sorted_by_path() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("z.ts"), "const z = 1;").unwrap();
        fs::write(dir.path().join("a.ts"), "const a = 2;").unwrap();

        let config = ScanConfig::default();
        let result = scan_directory(dir.path(), &config).unwrap();

        let paths: Vec<&str> = result.file_indices.iter().map(|fi| fi.path.as_str()).collect();
        assert!(paths[0] < paths[1], "file_indices should be sorted: {paths:?}");
    }

    // ── scan_file ────────────────────────────────────────────────────

    #[test]
    fn scan_file_returns_file_index() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("func.ts");
        fs::write(&file, "export function add(a: number, b: number) { return a + b; }").unwrap();

        let fi = scan_file(&file, dir.path(), FunctionKindsFilter::All).unwrap();

        assert!(fi.path.ends_with("func.ts"));
        assert!(!fi.functions.is_empty());
        assert_eq!(fi.parse_errors, 0);
    }

    #[test]
    fn scan_file_detects_arrow_function() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("arrow.ts");
        fs::write(&file, "export const double = (n: number) => n * 2;").unwrap();

        let fi = scan_file(&file, dir.path(), FunctionKindsFilter::All).unwrap();

        assert!(
            fi.functions.iter().any(|f| f.kind == types::FunctionKind::Arrow),
            "expected an arrow function in {fi:?}"
        );
    }

    #[test]
    fn scan_file_reports_exports() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("exp.ts");
        fs::write(&file, "export const FOO = 42;\nexport default function bar() {}").unwrap();

        let fi = scan_file(&file, dir.path(), FunctionKindsFilter::All).unwrap();

        let export_names: Vec<&str> = fi.exports.iter().map(|e| e.name.as_str()).collect();
        assert!(export_names.contains(&"FOO"), "exports: {export_names:?}");
        assert!(export_names.contains(&"bar"), "exports: {export_names:?}");
    }
}
