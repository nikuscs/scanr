use std::fs;
use std::io::Write;

use anyhow::{Context, Result};

use crate::index::db;

pub async fn run(root: &str) -> Result<()> {
    let project =
        fs::canonicalize(root).context("Cannot resolve project root")?.display().to_string();

    let pool = db::connect().await?;
    let deleted = db::clear_project(&pool, &project).await?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "Cleared {deleted} chunks for {project}")?;

    pool.close().await;
    Ok(())
}
