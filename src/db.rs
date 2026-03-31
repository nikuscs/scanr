use anyhow::{Context, Result};
use pgvector::Vector;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

const DEFAULT_DB_URL: &str = "postgresql://postgres@localhost/code_index";

pub fn db_url() -> String {
    std::env::var("CODE_INDEX_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DB_URL.to_string())
}

fn db_name() -> String {
    let url = db_url();
    url.rfind('/').map_or_else(
        || "code_index".to_string(),
        |idx| url[idx + 1..].split('?').next().unwrap_or("code_index").to_string(),
    )
}

fn admin_url() -> String {
    let url = db_url();
    if let Some(idx) = url.rfind('/') {
        let base = &url[..idx];
        let query = url[idx..].find('?').map_or("", |q| &url[idx + q..]);
        format!("{base}/postgres{query}")
    } else {
        url
    }
}

pub async fn ensure_postgres() -> Result<()> {
    if PgPoolOptions::new().max_connections(1).connect(&admin_url()).await.is_ok() {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        tracing::info!("PostgreSQL not reachable — attempting install via Homebrew...");

        let brew_check = Command::new("brew").args(["list", "postgresql@18"]).output();
        let already_installed = brew_check.as_ref().is_ok_and(|output| output.status.success());

        if already_installed {
            tracing::info!("PostgreSQL is installed but not running, starting...");
        } else {
            tracing::info!("Installing postgresql@18 via Homebrew...");
            let status = Command::new("brew")
                .args(["install", "postgresql@18"])
                .status()
                .context("Failed to install PostgreSQL (is Homebrew installed?)")?;
            if !status.success() {
                anyhow::bail!("Homebrew install failed");
            }
        }

        // Ensure pgvector extension is available
        let pgvector_check = Command::new("brew").args(["list", "pgvector"]).output();
        if !pgvector_check.as_ref().is_ok_and(|output| output.status.success()) {
            tracing::info!("Installing pgvector via Homebrew...");
            let status = Command::new("brew")
                .args(["install", "pgvector"])
                .status()
                .context("Failed to install pgvector")?;
            if !status.success() {
                anyhow::bail!("pgvector install failed");
            }
        }

        Command::new("brew")
            .args(["services", "start", "postgresql@18"])
            .status()
            .context("Failed to start PostgreSQL")?;

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url())
        .await
        .context("Cannot connect to PostgreSQL. Install and start it first.")?;

    Ok(())
}

pub async fn ensure_database() -> Result<()> {
    let name = db_name();
    let admin = PgPoolOptions::new().max_connections(1).connect(&admin_url()).await?;

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&name)
            .fetch_one(&admin)
            .await?;

    if !exists {
        // CREATE DATABASE doesn't support $1 bind params, so we validate the name
        sqlx::query(&format!("CREATE DATABASE \"{name}\"")).execute(&admin).await?;
        tracing::info!("\u{2713} Created database {name}");
    }

    admin.close().await;
    Ok(())
}

pub async fn connect() -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url())
        .await
        .context("Failed to connect to code_index database")
}

pub async fn setup_schema(pool: &PgPool) -> Result<()> {
    sqlx::query("CREATE EXTENSION IF NOT EXISTS vector").execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS code_index_chunks (
            id         BIGSERIAL PRIMARY KEY,
            content    TEXT NOT NULL,
            metadata   JSONB NOT NULL DEFAULT '{}',
            embedding  vector(3072)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS code_index_hashes (
            project      TEXT NOT NULL,
            file_path    TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            indexed_at   TIMESTAMPTZ DEFAULT NOW(),
            PRIMARY KEY  (project, file_path)
        )",
    )
    .execute(pool)
    .await?;

    let idx_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_indexes WHERE indexname = 'code_index_chunks_embedding_idx')",
    )
    .fetch_one(pool)
    .await?;

    if !idx_exists {
        sqlx::query(
            "CREATE INDEX code_index_chunks_embedding_idx ON code_index_chunks
             USING hnsw (embedding vector_cosine_ops)",
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn upsert_hash(pool: &PgPool, project: &str, file_path: &str, hash: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO code_index_hashes (project, file_path, content_hash, indexed_at)
         VALUES ($1, $2, $3, NOW())
         ON CONFLICT (project, file_path) DO UPDATE
           SET content_hash = EXCLUDED.content_hash, indexed_at = NOW()",
    )
    .bind(project)
    .bind(file_path)
    .bind(hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_stored_hash(
    pool: &PgPool,
    project: &str,
    file_path: &str,
) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT content_hash FROM code_index_hashes WHERE project = $1 AND file_path = $2",
    )
    .bind(project)
    .bind(file_path)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

pub async fn get_all_hashes(pool: &PgPool, project: &str) -> Result<Vec<(String, String)>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT file_path, content_hash FROM code_index_hashes WHERE project = $1")
            .bind(project)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

pub async fn delete_file_chunks(pool: &PgPool, project: &str, file_path: &str) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM code_index_chunks
         WHERE metadata->>'project' = $1 AND metadata->>'source' = $2",
    )
    .bind(project)
    .bind(file_path)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn insert_chunks(
    pool: &PgPool,
    project: &str,
    file_path: &str,
    language: &str,
    chunks: &[String],
    embeddings: &[Vec<f32>],
) -> Result<()> {
    for (content, embedding) in chunks.iter().zip(embeddings.iter()) {
        let metadata = serde_json::json!({
            "project": project,
            "source": file_path,
            "language": language,
        });

        sqlx::query(
            "INSERT INTO code_index_chunks (content, metadata, embedding)
             VALUES ($1, $2, $3)",
        )
        .bind(content)
        .bind(&metadata)
        .bind(Vector::from(embedding.clone()))
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub struct SearchResult {
    pub content: String,
    pub source: String,
    pub language: String,
    pub score: f64,
}

#[allow(clippy::option_if_let_else)]
pub async fn search_similar(
    pool: &PgPool,
    project: &str,
    embedding: &[f32],
    limit: usize,
    threshold: f64,
    lang: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let vec = Vector::from(embedding.to_vec());

    let query = if let Some(lang) = lang {
        sqlx::query(
            "SELECT content, metadata, 1 - (embedding <=> $1::vector) AS score
             FROM code_index_chunks
             WHERE metadata->>'project' = $2 AND metadata->>'language' = $4
             ORDER BY embedding <=> $1::vector
             LIMIT $3",
        )
        .bind(&vec)
        .bind(project)
        .bind(limit as i64)
        .bind(lang)
    } else {
        sqlx::query(
            "SELECT content, metadata, 1 - (embedding <=> $1::vector) AS score
             FROM code_index_chunks
             WHERE metadata->>'project' = $2
             ORDER BY embedding <=> $1::vector
             LIMIT $3",
        )
        .bind(&vec)
        .bind(project)
        .bind(limit as i64)
    };

    let rows = query.fetch_all(pool).await?;

    let results = rows
        .into_iter()
        .filter_map(|row| {
            let score: f64 = row.get("score");
            if score < threshold {
                return None;
            }
            let content: String = row.get("content");
            let metadata: serde_json::Value = row.get("metadata");
            let source = metadata["source"].as_str().unwrap_or("").to_string();
            let language = metadata["language"].as_str().unwrap_or("").to_string();
            Some(SearchResult { content, source, language, score })
        })
        .collect();

    Ok(results)
}

pub async fn chunk_count(pool: &PgPool, project: &str) -> Result<i64> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM code_index_chunks WHERE metadata->>'project' = $1")
            .bind(project)
            .fetch_one(pool)
            .await?;
    Ok(count.0)
}

pub async fn file_count(pool: &PgPool, project: &str) -> Result<i64> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT metadata->>'source') FROM code_index_chunks WHERE metadata->>'project' = $1",
    )
    .bind(project)
    .fetch_one(pool)
    .await?;
    Ok(count.0)
}

pub async fn clear_project(pool: &PgPool, project: &str) -> Result<i64> {
    let result = sqlx::query("DELETE FROM code_index_chunks WHERE metadata->>'project' = $1")
        .bind(project)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM code_index_hashes WHERE project = $1")
        .bind(project)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() as i64)
}
