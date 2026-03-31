use std::io::Write;

use anyhow::Result;

use crate::index::db;

pub async fn run() -> Result<()> {
    let pool = db::connect().await?;
    let projects = db::list_projects(&pool).await?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if projects.is_empty() {
        writeln!(out, "No indexed projects.")?;
        pool.close().await;
        return Ok(());
    }

    for p in &projects {
        let embed = p
            .embedding
            .as_ref()
            .map_or_else(|| "unknown".to_string(), |e| format!("{} ({}d)", e.spec(), e.dimensions));
        let updated = p.updated_at.as_deref().unwrap_or("—");
        writeln!(out, "{}", p.project)?;
        writeln!(
            out,
            "  Files: {}  Chunks: {}  Embed: {}  Updated: {}",
            p.files, p.chunks, embed, updated
        )?;
    }

    pool.close().await;
    Ok(())
}
