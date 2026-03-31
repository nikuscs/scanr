use anyhow::Result;
use std::io::Write;

use crate::cli::ScanArgs;
use crate::scan::output;
use crate::scan::types::{ScanResult, Stats};
use crate::scan::{self, ScanConfig};

#[allow(clippy::unused_async)]
pub async fn run(args: &ScanArgs) -> Result<()> {
    let root = std::path::Path::new(&args.root);

    let result = if let Some(file_path) = &args.file {
        // Single file mode
        let path = std::path::Path::new(file_path);
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let fi = scan::scan_file(path, &canonical_root, args.function_kinds)?;

        ScanResult {
            ver: 1,
            root: root.to_string_lossy().to_string(),
            stats: Stats {
                files: 1,
                parsed: 1,
                skipped: 0,
                errors: usize::from(fi.parse_errors > 0),
            },
            file_indices: vec![fi],
            errors: Vec::new(),
        }
    } else {
        // Directory scan
        let config = ScanConfig {
            extensions: args.include.clone(),
            exclude: args.exclude.clone(),
            max_bytes: args.max_bytes,
            function_kinds: args.function_kinds,
        };
        scan::scan_directory(root, &config)?
    };

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    output::write_result(&result, args.mode, &mut handle)?;
    handle.write_all(b"\n")?;

    Ok(())
}
