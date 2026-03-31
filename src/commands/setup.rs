use std::io::Write;

use anyhow::Result;

use crate::db;

pub async fn run() -> Result<()> {
    db::ensure_postgres().await?;
    db::ensure_database().await?;

    let pool = db::connect().await?;
    db::setup_schema(&pool).await?;

    pool.close().await;

    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "\u{2713} Schema ready")?;
    writeln!(stderr, "  Next: run `scanr index` to start indexing")?;
    Ok(())
}
