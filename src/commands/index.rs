use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::{chunk, db, embed, git};

pub async fn run(root: &str, single_file: Option<String>, force: bool) -> Result<()> {
    let project =
        fs::canonicalize(root).context("Cannot resolve project root")?.display().to_string();
    let root_path = PathBuf::from(&project);

    let embedder = embed::EmbedClient::new()?;
    let pool = db::connect().await?;
    let mut stderr = std::io::stderr().lock();

    let files = if let Some(ref file) = single_file {
        let rel = pathdiff(&root_path, file);
        writeln!(stderr, "Re-indexing {rel}...")?;
        vec![rel]
    } else {
        let files = git::list_files(&root_path)?;
        writeln!(stderr, "Found {} indexable files, splitting...", files.len())?;
        files
    };

    let mut added: u64 = 0;
    let mut skipped: u64 = 0;
    let mut updated: u64 = 0;
    let mut deleted: u64 = 0;

    let mut file_chunks: Vec<FileChunks> = Vec::new();

    for rel_path in &files {
        let abs_path = root_path.join(rel_path);
        let Ok(content) = fs::read_to_string(&abs_path) else {
            writeln!(stderr, "Skipping {rel_path} (unreadable)")?;
            continue;
        };

        let hash = sha256(&content);
        if !force && single_file.is_none() {
            if let Some(stored) = db::get_stored_hash(&pool, &project, rel_path).await? {
                if stored == hash {
                    skipped += 1;
                    continue;
                }
            }
        }

        let ext = git::ext_for_path(rel_path);
        let language = git::lang_for_ext(rel_path);

        let chunks = if language == "markdown" {
            chunk::chunk_markdown(&content)
        } else {
            chunk::chunk_code(&content, ext)?
        };

        if chunks.is_empty() {
            continue;
        }

        let del = db::delete_file_chunks(&pool, &project, rel_path).await?;
        if del > 0 {
            deleted += del;
            updated += 1;
        } else {
            added += 1;
        }

        file_chunks.push(FileChunks {
            rel_path: rel_path.clone(),
            language: language.to_string(),
            chunks,
            hash,
        });
    }

    if file_chunks.is_empty() {
        writeln!(stderr, "No documents to index.")?;
        pool.close().await;
        return Ok(());
    }

    let all_texts: Vec<String> = file_chunks.iter().flat_map(|fc| fc.chunks.clone()).collect();
    writeln!(stderr, "{} chunks — embedding and storing...", all_texts.len())?;

    // Drop stderr lock before the potentially long embed call
    drop(stderr);

    let all_embeddings = embedder.embed_batch(&all_texts).await?;

    let mut embed_idx = 0;
    for fc in &file_chunks {
        let chunk_count = fc.chunks.len();
        let embeddings = &all_embeddings[embed_idx..embed_idx + chunk_count];
        embed_idx += chunk_count;

        db::insert_chunks(&pool, &project, &fc.rel_path, &fc.language, &fc.chunks, embeddings)
            .await?;

        db::upsert_hash(&pool, &project, &fc.rel_path, &fc.hash).await?;
    }

    if force && single_file.is_none() {
        let indexed: HashSet<String> = files.iter().cloned().collect();
        let stored = db::get_all_hashes(&pool, &project).await?;
        for (file_path, _) in &stored {
            if !indexed.contains(file_path.as_str()) {
                let del = db::delete_file_chunks(&pool, &project, file_path).await?;
                deleted += del;
            }
        }
    }

    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "+{added} added  ={skipped} skipped  ~{updated} updated  -{deleted} deleted")?;

    pool.close().await;
    Ok(())
}

struct FileChunks {
    rel_path: String,
    language: String,
    chunks: Vec<String>,
    hash: String,
}

fn sha256(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn pathdiff(root: &Path, file: &str) -> String {
    let abs = if Path::new(file).is_absolute() { PathBuf::from(file) } else { root.join(file) };

    abs.strip_prefix(root).map_or_else(|_| file.to_string(), |p| p.display().to_string())
}
