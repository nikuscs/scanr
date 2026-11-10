//! Git plumbing shared by the commands that compare against a base revision.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};

/// A base revision resolved for an analysis root inside a Git worktree.
pub struct Base {
    /// Canonical analysis root.
    pub root: PathBuf,
    pub repo_root: PathBuf,
    /// Analysis root relative to the worktree root.
    pub root_prefix: PathBuf,
    /// Commit the requested base resolves to.
    pub commit: String,
}

pub fn resolve_base(root: &Path, base: &str) -> Result<Base> {
    let root = root.canonicalize().context("Cannot resolve project root for --base")?;
    require_worktree(&root)?;
    let repo_root_output = run_git(&root, ["rev-parse", "--show-toplevel"], "locate Git worktree")?;
    let repo_root_text = utf8_stdout(&repo_root_output, "Git worktree path")?;
    let repo_root = PathBuf::from(repo_root_text.trim())
        .canonicalize()
        .with_context(|| format!("cannot resolve Git worktree root `{}`", repo_root_text.trim()))?;
    let root_prefix = root
        .strip_prefix(&repo_root)
        .map_err(|_| anyhow!("analysis root is not inside the resolved Git worktree"))?
        .to_path_buf();

    let revision = format!("{base}^{{commit}}");
    let resolved_output = run_git(
        &root,
        ["rev-parse", "--verify", "--end-of-options", revision.as_str()],
        &format!("resolve --base `{base}`"),
    )?;
    let commit = utf8_stdout(&resolved_output, "resolved Git base")?.trim().to_string();
    Ok(Base { root, repo_root, root_prefix, commit })
}

impl Base {
    /// Contents of a root-relative path at the base commit.
    pub fn read(&self, path: &str) -> Result<String> {
        let repo_path = self.root_prefix.join(path).to_string_lossy().replace('\\', "/");
        let object = format!("{}:{repo_path}", self.commit);
        let output = run_git(
            &self.repo_root,
            ["show", "--end-of-options", object.as_str()],
            &format!("read `{path}` at the base revision"),
        )?;
        Ok(utf8_stdout(&output, "base file")?.to_string())
    }
}

fn require_worktree(root: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("--no-pager")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .context("failed to execute Git for --base")?;
    if !output.status.success() || output.stdout != b"true\n" {
        bail!("--base requires a Git worktree at `{}`", root.display());
    }
    Ok(())
}

pub fn run_git<'a>(
    root: &Path,
    args: impl IntoIterator<Item = &'a str>,
    operation: &str,
) -> Result<Output> {
    let output = Command::new("git")
        .arg("--no-pager")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to execute Git while attempting to {operation}"))?;
    check_git_output(output, operation)
}

pub fn run_git_owned(root: &Path, args: &[String], operation: &str) -> Result<Output> {
    let output = Command::new("git")
        .arg("--no-pager")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to execute Git while attempting to {operation}"))?;
    check_git_output(output, operation)
}

fn check_git_output(output: Output, operation: &str) -> Result<Output> {
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("failed to {operation}: {}", stderr.trim());
}

pub fn utf8_stdout<'a>(output: &'a Output, label: &str) -> Result<&'a str> {
    std::str::from_utf8(&output.stdout).with_context(|| format!("{label} is not valid UTF-8"))
}
