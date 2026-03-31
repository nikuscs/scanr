use std::io::Write;

use anyhow::Result;

use crate::cli::SetupArgs;
use crate::index::db;

pub async fn run(args: &SetupArgs) -> Result<()> {
    let pg_version = if args.yes { args.pg_version } else { prompt_pg_version(args.pg_version)? };

    db::ensure_postgres(pg_version).await?;
    db::ensure_database().await?;

    let pool = db::connect().await?;
    db::setup_schema(&pool).await?;

    pool.close().await;

    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "\u{2713} Schema ready")?;
    writeln!(stderr, "  Next: run `scanr index` to start indexing")?;
    Ok(())
}

fn prompt_pg_version(default: u32) -> Result<u32> {
    let mut stderr = std::io::stderr().lock();
    write!(stderr, "PostgreSQL version to install [{default}]: ")?;
    stderr.flush()?;
    drop(stderr);

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();

    if trimmed.is_empty() {
        return Ok(default);
    }

    trimmed.parse::<u32>().map_err(|_| anyhow::anyhow!("Invalid version: {trimmed}"))
}
