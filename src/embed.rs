use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

const OPENAI_EMBED_URL: &str = "https://api.openai.com/v1/embeddings";
const MODEL: &str = "text-embedding-3-large";
const DIMENSIONS: u32 = 3072;
const MAX_BATCH_SIZE: usize = 100;
const MAX_RETRIES: u32 = 3;

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    dimensions: u32,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

pub struct EmbedClient {
    client: reqwest::Client,
    api_key: String,
}

impl EmbedClient {
    pub fn new() -> Result<Self> {
        let api_key = resolve_api_key().context(
            "OPENAI_API_KEY is not set.\nSet it in ~/.zshrc, ~/.bashrc, or a .env file in your project:\n  export OPENAI_API_KEY=sk-...",
        )?;

        let client = reqwest::Client::new();
        Ok(Self { client, api_key })
    }

    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut all_embeddings = Vec::with_capacity(texts.len());
        let batches: Vec<&[String]> = texts.chunks(MAX_BATCH_SIZE).collect();

        let pb = ProgressBar::new(batches.len() as u64);
        #[allow(clippy::literal_string_with_formatting_args)]
        let style = ProgressStyle::default_bar()
            .template("  Embedding [{bar:30}] {pos}/{len} batches")
            .expect("valid template")
            .progress_chars("=> ");
        pb.set_style(style);

        for batch in &batches {
            let embeddings = self.embed_with_retry(batch).await?;
            all_embeddings.extend(embeddings);
            pb.inc(1);
        }

        pb.finish_and_clear();
        Ok(all_embeddings)
    }

    async fn embed_with_retry(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let request_body = EmbedRequest { model: MODEL, input: texts, dimensions: DIMENSIONS };

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff =
                    std::time::Duration::from_millis(500 * u64::from(1u32 << (attempt - 1)));
                tracing::warn!("Retrying OpenAI API (attempt {}/{})", attempt + 1, MAX_RETRIES + 1);
                tokio::time::sleep(backoff).await;
            }

            let resp = self
                .client
                .post(OPENAI_EMBED_URL)
                .bearer_auth(&self.api_key)
                .json(&request_body)
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) if attempt < MAX_RETRIES && is_retryable_error(&e) => continue,
                Err(e) => return Err(e).context("OpenAI API request failed"),
            };

            let status = resp.status();

            if status.is_success() {
                let response: EmbedResponse =
                    resp.json().await.context("Failed to parse OpenAI response")?;
                return Ok(response.data.into_iter().map(|d| d.embedding).collect());
            }

            if attempt < MAX_RETRIES && is_retryable_status(status.as_u16()) {
                continue;
            }

            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error {status}: {body}");
        }

        anyhow::bail!("OpenAI API: max retries exceeded")
    }

    pub async fn embed_single(&self, text: &str) -> Result<Vec<f32>> {
        let texts = vec![text.to_string()];
        let request_body = EmbedRequest { model: MODEL, input: &texts, dimensions: DIMENSIONS };

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff =
                    std::time::Duration::from_millis(500 * u64::from(1u32 << (attempt - 1)));
                tokio::time::sleep(backoff).await;
            }

            let resp = self
                .client
                .post(OPENAI_EMBED_URL)
                .bearer_auth(&self.api_key)
                .json(&request_body)
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) if attempt < MAX_RETRIES && is_retryable_error(&e) => continue,
                Err(e) => return Err(e).context("OpenAI API request failed"),
            };

            let status = resp.status();
            if status.is_success() {
                let response: EmbedResponse =
                    resp.json().await.context("Failed to parse OpenAI response")?;
                return response
                    .data
                    .into_iter()
                    .next()
                    .map(|d| d.embedding)
                    .context("Empty embedding response from OpenAI");
            }

            if attempt < MAX_RETRIES && is_retryable_status(status.as_u16()) {
                continue;
            }

            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error {status}: {body}");
        }

        anyhow::bail!("OpenAI API: max retries exceeded")
    }
}

const fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect()
}

/// Resolve `OPENAI_API_KEY`: check environment first, then walk up from cwd
/// looking for the closest `.env` file that defines it.
fn resolve_api_key() -> Option<String> {
    if let Ok(val) = std::env::var("OPENAI_API_KEY") {
        if !val.is_empty() {
            return Some(val);
        }
    }

    let mut dir = std::env::current_dir().ok()?;
    loop {
        let env_file = dir.join(".env");
        if let Some(val) = read_key_from_env_file(&env_file) {
            return Some(val);
        }
        if !dir.pop() {
            break;
        }
    }

    None
}

fn read_key_from_env_file(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("OPENAI_API_KEY") {
            let rest = rest.trim_start();
            if let Some(val) = rest.strip_prefix('=') {
                let val = val.trim();
                // Strip surrounding quotes if present
                let val = val
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .or_else(|| val.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                    .unwrap_or(val);
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}
