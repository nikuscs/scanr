use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::cli::IndexArgs;
use crate::{chunk, db, embed, git};

struct FileChunks {
    rel_path: String,
    language: String,
    chunks: Vec<String>,
    hash: String,
}

pub async fn run(args: &IndexArgs) -> Result<()> {
    let project =
        fs::canonicalize(&args.root).context("Cannot resolve project root")?.display().to_string();
    let root_path = PathBuf::from(&project);

    let embedder = embed::EmbedClient::new()?;
    let pool = db::connect().await?;
    let mut stderr = std::io::stderr().lock();

    let files = if let Some(ref file) = args.file {
        let rel = pathdiff(&root_path, file);
        writeln!(stderr, "Re-indexing {rel}...")?;
        vec![rel]
    } else {
        let files = git::list_files(&root_path, args.gitignore.as_deref())?;
        writeln!(stderr, "Found {} indexable files, chunking...", files.len())?;
        files
    };
    drop(stderr);

    let chunk_config = chunk::ChunkConfig { size: args.chunk_size, overlap: args.chunk_overlap };

    // Parallel file reading + chunking with progress bar
    let pb = ProgressBar::new(files.len() as u64);
    #[allow(clippy::literal_string_with_formatting_args)]
    let style = ProgressStyle::default_bar()
        .template("  Chunking  [{bar:30}] {pos}/{len} files")
        .expect("valid template")
        .progress_chars("=> ");
    pb.set_style(style);

    let skipped = Mutex::new(0u64);
    let file_chunks: Vec<FileChunks> = files
        .par_iter()
        .filter_map(|rel_path| {
            let result = process_file(
                rel_path,
                &root_path,
                &project,
                &pool,
                args.force,
                args.file.is_some(),
                &chunk_config,
                &skipped,
            );
            pb.inc(1);
            match result {
                Ok(Some(fc)) => Some(Ok(fc)),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            }
        })
        .collect::<Result<Vec<_>>>()?;

    pb.finish_and_clear();

    let skipped = skipped.into_inner().unwrap_or(0);

    if file_chunks.is_empty() {
        let mut stderr = std::io::stderr().lock();
        writeln!(stderr, "No documents to index.")?;
        pool.close().await;
        return Ok(());
    }

    // Delete old chunks for files that will be re-indexed
    let mut added: u64 = 0;
    let mut updated: u64 = 0;
    let mut deleted: u64 = 0;

    for fc in &file_chunks {
        let del = db::delete_file_chunks(&pool, &project, &fc.rel_path).await?;
        if del > 0 {
            deleted += del;
            updated += 1;
        } else {
            added += 1;
        }
    }

    // Flatten all chunks for batch embedding
    let all_texts: Vec<String> = file_chunks.iter().flat_map(|fc| fc.chunks.clone()).collect();

    {
        let mut stderr = std::io::stderr().lock();
        writeln!(stderr, "  {} chunks from {} files", all_texts.len(), file_chunks.len())?;
    }

    let all_embeddings = embedder.embed_batch(&all_texts).await?;

    // Insert chunks with their embeddings
    let mut embed_idx = 0;
    for fc in &file_chunks {
        let n = fc.chunks.len();
        let embeddings = &all_embeddings[embed_idx..embed_idx + n];
        embed_idx += n;

        db::insert_chunks(&pool, &project, &fc.rel_path, &fc.language, &fc.chunks, embeddings)
            .await?;
        db::upsert_hash(&pool, &project, &fc.rel_path, &fc.hash).await?;
    }

    // Clean up hashes for deleted files (force mode)
    if args.force && args.file.is_none() {
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
    writeln!(
        stderr,
        "\u{2713} +{added} added  ={skipped} skipped  ~{updated} updated  -{deleted} deleted"
    )?;

    pool.close().await;
    Ok(())
}

#[allow(clippy::significant_drop_tightening, clippy::too_many_arguments)]
fn process_file(
    rel_path: &str,
    root_path: &Path,
    project: &str,
    pool: &sqlx::PgPool,
    force: bool,
    is_single: bool,
    chunk_config: &chunk::ChunkConfig,
    skipped: &Mutex<u64>,
) -> Result<Option<FileChunks>> {
    let abs_path = root_path.join(rel_path);
    let Ok(content) = fs::read_to_string(&abs_path) else {
        return Ok(None);
    };

    let hash = sha256(&content);

    // Check hash for incremental skip (blocking runtime call from rayon)
    if !force && !is_single {
        let rt = tokio::runtime::Handle::current();
        if let Ok(Some(stored)) = rt.block_on(db::get_stored_hash(pool, project, rel_path)) {
            if stored == hash {
                let mut s = skipped.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                *s += 1;
                return Ok(None);
            }
        }
    }

    let ext = git::ext_for_path(rel_path);
    let language = git::lang_for_ext(rel_path);

    let chunks = if language == "markdown" {
        chunk::chunk_markdown(&content, chunk_config)
    } else {
        chunk::chunk_code(&content, ext, chunk_config)?
    };

    if chunks.is_empty() {
        return Ok(None);
    }

    Ok(Some(FileChunks {
        rel_path: rel_path.to_string(),
        language: language.to_string(),
        chunks,
        hash,
    }))
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
