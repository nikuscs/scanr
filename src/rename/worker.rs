use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const WORKER_BUNDLE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/rename-worker.mjs"));

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRequest {
    pub tsconfig: String,
    pub file: String,
    pub line: u32,
    pub name: String,
    pub new_name: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerResponse {
    pub ver: u8,
    pub status: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub files: Vec<String>,
    pub message: Option<String>,
}

pub fn run_worker(request: &WorkerRequest) -> Result<WorkerResponse> {
    let (runtime, args) = runtime_invocation()?;
    let mut child = Command::new(runtime)
        .args(args)
        .env("SCANR_RENAME_REQUEST", serde_json::to_string(request)?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn {runtime}"))?;

    let mut stdin = child.stdin.take().context("rename worker stdin is missing")?;
    stdin.write_all(WORKER_BUNDLE.as_bytes()).context("failed to write rename worker bundle")?;
    drop(stdin);

    let output = child.wait_with_output().context("failed to wait for rename worker")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let response: WorkerResponse = serde_json::from_str(stdout.trim()).with_context(|| {
        format!("rename worker returned invalid JSON (status {}): {stdout}", output.status)
    })?;
    if response.status == "error" {
        bail!("{}", response.message.as_deref().unwrap_or("rename worker failed"));
    }
    if !output.status.success() {
        bail!(
            "rename worker exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if response.status != "ok" {
        bail!("rename worker returned status {}", response.status);
    }
    Ok(response)
}

fn runtime_invocation() -> Result<(&'static str, &'static [&'static str])> {
    for (candidate, args) in [("bun", &["run", "-"][..]), ("node", &["--input-type=module"][..])] {
        if which(candidate) {
            return Ok((candidate, args));
        }
    }
    anyhow::bail!("scanr rename requires bun or node on PATH")
}

fn which(candidate: &str) -> bool {
    Command::new(candidate)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .is_ok_and(|status| status.success())
}
