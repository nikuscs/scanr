use std::collections::HashMap;
use std::fs;
use std::io::Write;

use anyhow::{Context, Result};

use crate::index::{db, embed, git};

pub async fn run(
    query: &str,
    root: &str,
    limit: usize,
    threshold: f64,
    lang: Option<String>,
    files_only: bool,
    as_json: bool,
) -> Result<()> {
    if query.trim().is_empty() {
        anyhow::bail!(
            "Usage: scanr search <query> [--root .] [--limit 10] [--threshold 0.0] [--lang ts] [--files-only] [--json]"
        );
    }

    let root_path = std::path::PathBuf::from(
        fs::canonicalize(root).context("Cannot resolve project root")?.display().to_string(),
    );

    let embedder = embed::EmbedClient::new(Some(&root_path))?;
    let project = root_path.display().to_string();
    let pool = db::connect().await?;

    let query_embedding = embedder.embed_single(query).await?;

    // Map short extension (e.g. "ts") to full language name (e.g. "typescript")
    let resolved_lang = lang.as_deref().map(|l| {
        let full = git::lang_for_ext(&format!("_.{l}"));
        if full.is_empty() { l.to_string() } else { full.to_string() }
    });

    let results = db::search_similar(
        &pool,
        &project,
        &query_embedding,
        limit,
        threshold,
        resolved_lang.as_deref(),
    )
    .await?;

    if !as_json {
        let stale = stale_count(&pool, &project).await.unwrap_or(0);
        if stale > 0 {
            let mut stderr = std::io::stderr().lock();
            writeln!(
                stderr,
                "\u{26a0} {stale} file(s) changed since last index — run `index` to update"
            )
            .ok();
        }
    }

    if results.is_empty() {
        if as_json {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            write!(out, "[]")?;
        } else {
            let mut stderr = std::io::stderr().lock();
            writeln!(stderr, "No results. Run `scanr index` first.")?;
        }
        pool.close().await;
        return Ok(());
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if as_json {
        let json_results: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "file": r.source,
                    "language": r.language,
                    "score": (r.score * 1000.0).round() / 1000.0,
                    "content": r.content,
                })
            })
            .collect();

        write!(out, "{}", serde_json::to_string_pretty(&json_results)?)?;
    } else if files_only {
        let mut best: HashMap<String, f64> = HashMap::new();
        for r in &results {
            let entry = best.entry(r.source.clone()).or_insert(0.0);
            if r.score > *entry {
                *entry = r.score;
            }
        }
        let mut pairs: Vec<_> = best.into_iter().collect();
        pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (file, score) in pairs {
            writeln!(out, "{}  ({:.1}%)", file, score * 100.0)?;
        }
    } else {
        for r in &results {
            let sim = r.score * 100.0;
            let content = if r.content.len() > 1200 {
                format!("{}\n...", &r.content[..1200])
            } else {
                r.content.clone()
            };
            writeln!(out, "\n### {}  ({:.1}%)", r.source, sim)?;
            writeln!(out, "```{}", r.language)?;
            writeln!(out, "{content}")?;
            writeln!(out, "```")?;
        }
    }

    pool.close().await;
    Ok(())
}

async fn stale_count(pool: &sqlx::PgPool, project: &str) -> Result<usize> {
    use sha2::{Digest, Sha256};

    let stored = db::get_all_hashes(pool, project).await?;
    if stored.is_empty() {
        return Ok(0);
    }

    let stored_map: HashMap<String, String> = stored.into_iter().collect();
    let root = std::path::Path::new(project);
    let current_files = crate::index::git::list_files(root, None)?;
    let mut stale = 0;

    for rel_path in &current_files {
        let abs_path = root.join(rel_path);
        if let Ok(content) = fs::read_to_string(&abs_path) {
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            let hash = format!("{:x}", hasher.finalize());
            if stored_map.get(rel_path.as_str()).is_none_or(|h| *h != hash) {
                stale += 1;
            }
        }
    }

    let current_set: std::collections::HashSet<&str> =
        current_files.iter().map(String::as_str).collect();
    for key in stored_map.keys() {
        if !current_set.contains(key.as_str()) {
            stale += 1;
        }
    }

    Ok(stale)
}
