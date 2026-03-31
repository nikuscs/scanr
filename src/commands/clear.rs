use std::fs;
use std::io::Write;

use anyhow::{Context, Result};

use crate::index::{db, git};

pub async fn run(root: &str) -> Result<()> {
    let root_path = fs::canonicalize(root).context("Cannot resolve project root")?;
    let project = git::resolve_project_id(&root_path)?;

    let pool = db::connect().await?;
    let deleted = db::clear_project(&pool, &project).await?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "Cleared {deleted} chunks for {project}")?;

    pool.close().await;
    Ok(())
}
