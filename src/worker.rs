//! Runs the bundled TypeScript worker (rename, type-aware members) through bun or node.
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const WORKER_BUNDLE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/worker.mjs"));

/// Sends `{command, ...request}` to the worker and decodes its `{ver,status,...}` reply.
pub fn run_worker<T: DeserializeOwned>(command: &str, request: &impl Serialize) -> Result<T> {
    let mut payload = serde_json::to_value(request)?;
    payload["command"] = Value::from(command);
    let (runtime, args) = runtime_invocation(command)?;
    let mut child = Command::new(runtime)
        .args(args)
        .env("SCANR_WORKER_REQUEST", payload.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn {runtime}"))?;

    let mut stdin = child.stdin.take().context("scanr worker stdin is missing")?;
    stdin.write_all(WORKER_BUNDLE.as_bytes()).context("failed to write scanr worker bundle")?;
    drop(stdin);

    let output = child.wait_with_output().context("failed to wait for scanr worker")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let response: Value = serde_json::from_str(stdout.trim()).with_context(|| {
        format!(
            "{command} worker returned invalid JSON (status {}): {stdout}{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    match response.get("status").and_then(Value::as_str) {
        Some("error") => bail!(
            "{}",
            response.get("message").and_then(Value::as_str).unwrap_or("scanr worker failed")
        ),
        _ if !output.status.success() => bail!(
            "{command} worker exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
        Some("ok") => Ok(serde_json::from_value(response)?),
        status => bail!("{command} worker returned status {status:?}"),
    }
}

fn runtime_invocation(command: &str) -> Result<(&'static str, &'static [&'static str])> {
    for (candidate, args) in [("bun", &["run", "-"][..]), ("node", &["--input-type=module"][..])] {
        if which(candidate) {
            return Ok((candidate, args));
        }
    }
    bail!("scanr {command} requires bun or node on PATH")
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
