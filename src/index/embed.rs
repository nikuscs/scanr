use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

const OPENAI_EMBED_URL: &str = "https://api.openai.com/v1/embeddings";
const MODEL: &str = "text-embedding-3-large";
const DIMENSIONS: u32 = 3072;
const MAX_BATCH_SIZE: usize = 100;
const MAX_RETRIES: u32 = 3;
/// Safety limit for chunk size before embedding. `OpenAI` `text-embedding-3-large`
/// has an 8192 token context window. For code/prose the ratio is ~3-4 chars/token,
/// but for JSON/data with short keys it can be as low as ~1.5 chars/token.
/// 12 000 chars is safe even for the worst-case tokenization ratio.
const MAX_CHUNK_CHARS: usize = 12_000;

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
    pub fn new(project_root: Option<&Path>) -> Result<Self> {
        let api_key = resolve_api_key(project_root).context(
            "OPENAI_API_KEY is not set.\nSet it in ~/.zshrc, ~/.bashrc, or a .env file in your project:\n  export OPENAI_API_KEY=sk-...",
        )?;

        let client = reqwest::Client::new();
        Ok(Self { client, api_key })
    }

    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let truncated: Vec<String> = texts.iter().map(|t| truncate_chunk(t)).collect();
        let mut all_embeddings = Vec::with_capacity(truncated.len());
        let batches: Vec<&[String]> = truncated.chunks(MAX_BATCH_SIZE).collect();

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
        let texts = vec![truncate_chunk(text)];
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

/// Truncate a chunk to `MAX_CHUNK_CHARS`, splitting at a char boundary.
fn truncate_chunk(text: &str) -> String {
    if text.len() <= MAX_CHUNK_CHARS {
        return text.to_string();
    }
    // Find a valid char boundary at or before MAX_CHUNK_CHARS
    let mut end = MAX_CHUNK_CHARS;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Resolve `OPENAI_API_KEY`: check environment first, then walk up from the
/// project root (if provided) and cwd looking for the closest `.env` file.
fn resolve_api_key(project_root: Option<&Path>) -> Option<String> {
    if let Ok(val) = std::env::var("OPENAI_API_KEY") {
        if !val.is_empty() {
            return Some(val);
        }
    }

    // Search from project root first (the --root flag target)
    if let Some(root) = project_root {
        if let Some(val) = walk_env_files(root) {
            return Some(val);
        }
    }

    // Fall back to cwd
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(val) = walk_env_files(&cwd) {
            return Some(val);
        }
    }

    None
}

fn walk_env_files(start: &Path) -> Option<String> {
    let mut dir = start.to_path_buf();
    for _ in 0..6 {
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

pub fn read_key_from_env_file(path: &Path) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_env_file(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
        let path = dir.join(".env");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn reads_unquoted_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=sk-test123\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-test123".to_string()));
    }

    #[test]
    fn reads_double_quoted_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"sk-quoted\"\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-quoted".to_string()));
    }

    #[test]
    fn reads_single_quoted_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY='sk-single'\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-single".to_string()));
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let content = "# This is a comment\n\nOTHER_KEY=foo\nOPENAI_API_KEY=sk-after\n";
        let path = write_env_file(dir.path(), content);
        assert_eq!(read_key_from_env_file(&path), Some("sk-after".to_string()));
    }

    #[test]
    fn returns_none_if_key_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OTHER_VAR=hello\n");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn returns_none_for_empty_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\n");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn returns_none_for_nonexistent_file() {
        let path = std::path::PathBuf::from("/tmp/does-not-exist-scanr-test/.env");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn handles_spaces_around_equals() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY = sk-spaced\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-spaced".to_string()));
    }

    // --- Edge cases for .env parsing ---

    #[test]
    fn trailing_comment_is_included_in_unquoted_value() {
        // The parser does NOT strip inline comments for unquoted values,
        // so a trailing `# comment` becomes part of the value.
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=sk-abc123 # my key\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-abc123 # my key".to_string()));
    }

    #[test]
    fn trailing_comment_inside_double_quotes_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"sk-abc # not a comment\"\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-abc # not a comment".to_string()));
    }

    #[test]
    fn export_prefix_is_not_recognized() {
        // The parser looks for lines starting with "OPENAI_API_KEY", so
        // `export OPENAI_API_KEY=...` does NOT match.
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "export OPENAI_API_KEY=sk-exported\n");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn value_containing_equals_signs() {
        // Values with `=` in them (e.g., base64 tokens) should work because
        // we only split on the first `=`.
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=sk-abc==def=\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-abc==def=".to_string()));
    }

    #[test]
    fn value_with_equals_inside_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"sk-a=b=c\"\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-a=b=c".to_string()));
    }

    #[test]
    fn value_with_special_characters() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=sk-!@$%^&*()_+\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-!@$%^&*()_+".to_string()));
    }

    #[test]
    fn value_with_special_characters_in_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"sk-special!@#$%^&*()\"\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-special!@#$%^&*()".to_string()));
    }

    #[test]
    fn empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn file_with_only_comments() {
        let dir = tempfile::tempdir().unwrap();
        let content = "# This is a comment\n# Another comment\n# OPENAI_API_KEY=sk-nope\n";
        let path = write_env_file(dir.path(), content);
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn file_with_only_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "\n\n\n\n");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn windows_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path =
            write_env_file(dir.path(), "OTHER=foo\r\nOPENAI_API_KEY=sk-crlf\r\nANOTHER=bar\r\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-crlf".to_string()));
    }

    #[test]
    fn windows_line_endings_quoted() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"sk-crlf-quoted\"\r\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-crlf-quoted".to_string()));
    }

    #[test]
    fn no_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=sk-noterminator");
        assert_eq!(read_key_from_env_file(&path), Some("sk-noterminator".to_string()));
    }

    #[test]
    fn key_among_many_vars() {
        let dir = tempfile::tempdir().unwrap();
        let content = "\
DB_HOST=localhost
DB_PORT=5432
OPENAI_API_KEY=sk-middle
REDIS_URL=redis://localhost
";
        let path = write_env_file(dir.path(), content);
        assert_eq!(read_key_from_env_file(&path), Some("sk-middle".to_string()));
    }

    #[test]
    fn first_occurrence_wins() {
        let dir = tempfile::tempdir().unwrap();
        let content = "OPENAI_API_KEY=sk-first\nOPENAI_API_KEY=sk-second\n";
        let path = write_env_file(dir.path(), content);
        assert_eq!(read_key_from_env_file(&path), Some("sk-first".to_string()));
    }

    #[test]
    fn empty_quoted_value_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"\"\n");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn empty_single_quoted_value_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=''\n");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn mismatched_quotes_treated_as_unquoted() {
        // Opening double-quote with closing single-quote should not strip.
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"sk-mismatch'\n");
        assert_eq!(read_key_from_env_file(&path), Some("\"sk-mismatch'".to_string()));
    }

    #[test]
    fn value_with_spaces_inside_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"sk key with spaces\"\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk key with spaces".to_string()));
    }

    #[test]
    fn leading_whitespace_on_line_is_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "  \t OPENAI_API_KEY=sk-indented\n");
        assert_eq!(read_key_from_env_file(&path), Some("sk-indented".to_string()));
    }

    #[test]
    fn key_suffix_does_not_match() {
        // `OPENAI_API_KEY_V2` starts with `OPENAI_API_KEY` — make sure the
        // parser still picks it up (it does, because it only checks prefix).
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY_V2=sk-v2\n");
        // The parser sees the prefix "OPENAI_API_KEY", remainder is "_V2=sk-v2",
        // trim_start gives "_V2=sk-v2", strip_prefix('=') fails → None.
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn value_only_whitespace_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=   \n");
        assert_eq!(read_key_from_env_file(&path), None);
    }

    #[test]
    fn value_only_whitespace_inside_quotes_returns_some() {
        // Quoted whitespace-only value: after stripping quotes, the value
        // is "   " which gets trimmed to empty → should return None.
        let dir = tempfile::tempdir().unwrap();
        let path = write_env_file(dir.path(), "OPENAI_API_KEY=\"   \"\n");
        // After trim() it's empty, but quotes are stripped after trim, so:
        // val = `"   "` → trim → `"   "` → strip quotes → `   ` → not empty → Some.
        // Actually, let's trace the code:
        //   rest = `=   "   "\n` (after line.trim() and strip_prefix("OPENAI_API_KEY"))
        //   Wait, line.trim() gives `OPENAI_API_KEY="   "`, rest after strip = `"   "`
        //   rest.trim_start() = `"   "`, strip_prefix('=')... no that's wrong.
        //   Let me re-read: strip_prefix("OPENAI_API_KEY") gives `="   "`,
        //   trim_start gives `="   "`, strip_prefix('=') gives `"   "`,
        //   val.trim() gives `"   "`, strip double quotes gives `   `,
        //   `   `.is_empty() is false → Some("   ").
        assert_eq!(read_key_from_env_file(&path), Some("   ".to_string()));
    }

    // --- Tests for is_retryable_status ---

    #[test]
    fn retryable_statuses() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(504));
    }

    #[test]
    fn non_retryable_statuses() {
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(201));
        assert!(!is_retryable_status(400));
        assert!(!is_retryable_status(401));
        assert!(!is_retryable_status(403));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(422));
        assert!(!is_retryable_status(501));
    }

    // --- Tests for truncate_chunk ---

    #[test]
    fn truncate_chunk_short_text_unchanged() {
        let text = "hello world";
        assert_eq!(truncate_chunk(text), text);
    }

    #[test]
    fn truncate_chunk_at_limit_unchanged() {
        let text = "x".repeat(MAX_CHUNK_CHARS);
        assert_eq!(truncate_chunk(&text).len(), MAX_CHUNK_CHARS);
    }

    #[test]
    fn truncate_chunk_oversized_is_truncated() {
        let text = "a".repeat(MAX_CHUNK_CHARS + 5000);
        let result = truncate_chunk(&text);
        assert_eq!(result.len(), MAX_CHUNK_CHARS);
    }

    #[test]
    fn truncate_chunk_respects_multibyte_boundary() {
        // Create string with multibyte chars near the boundary
        let mut text = "a".repeat(MAX_CHUNK_CHARS - 2);
        text.push('é'); // 2-byte char — now at MAX_CHUNK_CHARS
        text.push_str("overflow");
        let result = truncate_chunk(&text);
        assert!(result.len() <= MAX_CHUNK_CHARS);
        assert!(result.is_char_boundary(result.len()));
    }
}
