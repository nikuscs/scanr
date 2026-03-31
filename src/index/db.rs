use anyhow::{Context, Result};
use pgvector::Vector;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::index::embed::EmbeddingConfig;

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

pub async fn ensure_postgres(pg_version: u32) -> Result<()> {
    let _ = pg_version; // used only on macOS
    if PgPoolOptions::new().max_connections(1).connect(&admin_url()).await.is_ok() {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        let pkg = format!("postgresql@{pg_version}");
        tracing::info!("PostgreSQL not reachable — attempting install via Homebrew...");

        let brew_check = Command::new("brew").args(["list", &pkg]).output();
        let already_installed = brew_check.as_ref().is_ok_and(|output| output.status.success());

        if already_installed {
            tracing::info!("{pkg} is installed but not running, starting...");
        } else {
            tracing::info!("Installing {pkg} via Homebrew...");
            let status = Command::new("brew")
                .args(["install", &pkg])
                .status()
                .context("Failed to install PostgreSQL (is Homebrew installed?)")?;
            if !status.success() {
                anyhow::bail!("Homebrew install of {pkg} failed");
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
            .args(["services", "start", &pkg])
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
        "CREATE TABLE IF NOT EXISTS code_index_projects (
            project              TEXT PRIMARY KEY,
            embedding_provider   TEXT NOT NULL,
            embedding_model      TEXT NOT NULL,
            embedding_dimensions INTEGER NOT NULL,
            config_version       INTEGER NOT NULL DEFAULT 1,
            created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS code_index_chunks (
            id         BIGSERIAL PRIMARY KEY,
            content    TEXT NOT NULL,
            metadata   JSONB NOT NULL DEFAULT '{}',
            embedding  vector(2000)
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

pub async fn get_project_config(pool: &PgPool, project: &str) -> Result<Option<EmbeddingConfig>> {
    let row = sqlx::query(
        "SELECT embedding_provider, embedding_model, embedding_dimensions
         FROM code_index_projects
         WHERE project = $1",
    )
    .bind(project)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| EmbeddingConfig {
        provider: row.get("embedding_provider"),
        model: row.get("embedding_model"),
        dimensions: row.get::<i32, _>("embedding_dimensions") as u32,
    }))
}

pub async fn upsert_project_config(
    pool: &PgPool,
    project: &str,
    config: &EmbeddingConfig,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO code_index_projects (
            project, embedding_provider, embedding_model, embedding_dimensions, updated_at
         )
         VALUES ($1, $2, $3, $4, NOW())
         ON CONFLICT (project) DO UPDATE
           SET embedding_provider = EXCLUDED.embedding_provider,
               embedding_model = EXCLUDED.embedding_model,
               embedding_dimensions = EXCLUDED.embedding_dimensions,
               updated_at = NOW()",
    )
    .bind(project)
    .bind(&config.provider)
    .bind(&config.model)
    .bind(config.dimensions as i32)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn resolve_project_config(
    pool: &PgPool,
    project: &str,
    default_config: &EmbeddingConfig,
) -> Result<EmbeddingConfig> {
    if let Some(config) = get_project_config(pool, project).await? {
        return Ok(config);
    }

    if let Some(config) = infer_project_config_from_chunks(pool, project).await? {
        upsert_project_config(pool, project, &config).await?;
        return Ok(config);
    }

    if project_has_indexed_data(pool, project).await? {
        upsert_project_config(pool, project, default_config).await?;
        return Ok(default_config.clone());
    }

    Ok(default_config.clone())
}

pub async fn ensure_project_config(
    pool: &PgPool,
    project: &str,
    config: &EmbeddingConfig,
) -> Result<EmbeddingConfig> {
    let stored = resolve_project_config(pool, project, config).await?;

    if stored != *config {
        anyhow::bail!(
            "Project was indexed with {} ({}d), but this command requested {} ({}d). Use `scanr reindex --embedding {}` to switch embedding settings for this project.",
            stored.spec(),
            stored.dimensions,
            config.spec(),
            config.dimensions,
            config.spec()
        );
    }

    upsert_project_config(pool, project, config).await?;
    Ok(stored)
}

pub async fn project_has_indexed_data(pool: &PgPool, project: &str) -> Result<bool> {
    let has_chunks: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM code_index_chunks WHERE metadata->>'project' = $1)",
    )
    .bind(project)
    .fetch_one(pool)
    .await?;

    if has_chunks {
        return Ok(true);
    }

    let has_hashes: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM code_index_hashes WHERE project = $1)")
            .bind(project)
            .fetch_one(pool)
            .await?;

    Ok(has_hashes)
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
    config: &EmbeddingConfig,
    chunks: &[String],
    embeddings: &[Vec<f32>],
) -> Result<()> {
    for (content, embedding) in chunks.iter().zip(embeddings.iter()) {
        let padded = crate::index::embed::pad_embedding_for_storage(embedding)?;
        let metadata = serde_json::json!({
            "project": project,
            "source": file_path,
            "language": language,
            "embedding_provider": config.provider,
            "embedding_model": config.model,
            "embedding_dimensions": config.dimensions,
        });

        sqlx::query(
            "INSERT INTO code_index_chunks (content, metadata, embedding)
             VALUES ($1, $2, $3)",
        )
        .bind(content)
        .bind(&metadata)
        .bind(Vector::from(padded))
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
    let vec = Vector::from(crate::index::embed::pad_embedding_for_storage(embedding)?);

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

async fn infer_project_config_from_chunks(
    pool: &PgPool,
    project: &str,
) -> Result<Option<EmbeddingConfig>> {
    let row = sqlx::query(
        "SELECT metadata
         FROM code_index_chunks
         WHERE metadata->>'project' = $1
           AND metadata ? 'embedding_provider'
           AND metadata ? 'embedding_model'
           AND metadata ? 'embedding_dimensions'
         LIMIT 1",
    )
    .bind(project)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let metadata: serde_json::Value = row.get("metadata");
    let provider = metadata["embedding_provider"].as_str();
    let model = metadata["embedding_model"].as_str();
    let dimensions = metadata["embedding_dimensions"].as_u64();

    match (provider, model, dimensions) {
        (Some(provider), Some(model), Some(dimensions)) => Ok(Some(EmbeddingConfig {
            provider: provider.to_string(),
            model: model.to_string(),
            dimensions: dimensions as u32,
        })),
        _ => Ok(None),
    }
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

    sqlx::query("DELETE FROM code_index_projects WHERE project = $1")
        .bind(project)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() as i64)
}

pub struct ProjectSummary {
    pub project: String,
    pub chunks: i64,
    pub files: i64,
    pub embedding: Option<EmbeddingConfig>,
    pub updated_at: Option<String>,
}

pub async fn list_projects(pool: &PgPool) -> Result<Vec<ProjectSummary>> {
    let rows = sqlx::query(
        "SELECT
            p.project,
            COALESCE(c.chunks, 0) AS chunks,
            COALESCE(c.files, 0) AS files,
            p.embedding_provider,
            p.embedding_model,
            p.embedding_dimensions,
            TO_CHAR(p.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI') AS updated_at
         FROM code_index_projects p
         LEFT JOIN (
            SELECT metadata->>'project' AS project,
                   COUNT(*) AS chunks,
                   COUNT(DISTINCT metadata->>'source') AS files
            FROM code_index_chunks
            GROUP BY metadata->>'project'
         ) c ON c.project = p.project
         ORDER BY p.updated_at DESC",
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(ProjectSummary {
            project: row.get("project"),
            chunks: row.get("chunks"),
            files: row.get("files"),
            embedding: Some(EmbeddingConfig {
                provider: row.get("embedding_provider"),
                model: row.get("embedding_model"),
                dimensions: row.get::<i32, _>("embedding_dimensions") as u32,
            }),
            updated_at: row.get("updated_at"),
        });
    }
    Ok(results)
}
