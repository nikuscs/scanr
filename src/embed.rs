use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const OPENAI_EMBED_URL: &str = "https://api.openai.com/v1/embeddings";
const MODEL: &str = "text-embedding-3-large";
const DIMENSIONS: u32 = 3072;
const MAX_BATCH_SIZE: usize = 100;

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
        let api_key = std::env::var("OPENAI_API_KEY").context(
            "OPENAI_API_KEY is not set.\nAdd it to ~/.zshrc:  export OPENAI_API_KEY=sk-...",
        )?;

        let client = reqwest::Client::new();
        Ok(Self { client, api_key })
    }

    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut all_embeddings = Vec::with_capacity(texts.len());

        for batch in texts.chunks(MAX_BATCH_SIZE) {
            let request = EmbedRequest { model: MODEL, input: batch, dimensions: DIMENSIONS };

            let resp = self
                .client
                .post(OPENAI_EMBED_URL)
                .bearer_auth(&self.api_key)
                .json(&request)
                .send()
                .await
                .context("OpenAI API request failed")?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("OpenAI API error {status}: {body}");
            }

            let response: EmbedResponse =
                resp.json().await.context("Failed to parse OpenAI response")?;

            for data in response.data {
                all_embeddings.push(data.embedding);
            }
        }

        Ok(all_embeddings)
    }

    pub async fn embed_single(&self, text: &str) -> Result<Vec<f32>> {
        let texts = vec![text.to_string()];
        let mut embeddings = self.embed_batch(&texts).await?;
        embeddings.pop().context("Empty embedding response from OpenAI")
    }
}
