use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::{db, git};

pub async fn run(root: &str) -> Result<()> {
    let project =
        fs::canonicalize(root).context("Cannot resolve project root")?.display().to_string();

    let pool = db::connect().await?;
    let root_path = std::path::Path::new(project.as_str());

    let chunks = db::chunk_count(&pool, &project).await?;
    let files = db::file_count(&pool, &project).await?;
    let stored = db::get_all_hashes(&pool, &project).await?;

    let stored_map: HashMap<String, String> = stored.into_iter().collect();
    let current_files = git::list_files(root_path)?;
    let mut stale_files: Vec<String> = Vec::new();

    for rel_path in &current_files {
        let abs_path = root_path.join(rel_path);
        if let Ok(content) = fs::read_to_string(&abs_path) {
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            let hash = format!("{:x}", hasher.finalize());
            if stored_map.get(rel_path.as_str()).is_none_or(|h| *h != hash) {
                stale_files.push(rel_path.clone());
            }
        }
    }

    let current_set: HashSet<&str> = current_files.iter().map(String::as_str).collect();
    for key in stored_map.keys() {
        if !current_set.contains(key.as_str()) {
            stale_files.push(key.clone());
        }
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    writeln!(out, "Project : {project}")?;
    writeln!(out, "Files   : {files}")?;
    writeln!(out, "Chunks  : {chunks}")?;

    let stale_suffix = if stale_files.is_empty() { "" } else { " — run `index` to update" };
    writeln!(out, "Stale   : {}{stale_suffix}", stale_files.len())?;

    if !stale_files.is_empty() && stale_files.len() <= 20 {
        for f in &stale_files {
            writeln!(out, "  - {f}")?;
        }
    } else if stale_files.len() > 20 {
        for f in &stale_files[..20] {
            writeln!(out, "  - {f}")?;
        }
        writeln!(out, "  ... and {} more", stale_files.len() - 20)?;
    }

    pool.close().await;
    Ok(())
}
