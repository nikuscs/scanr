//! `scanr.json`: project rules that turn observations into checks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub const CONFIG_FILE: &str = "scanr.json";

#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Rules per file role, keyed as `scanr roles` names them: `.layer`, `index`.
    #[serde(default)]
    pub roles: BTreeMap<String, RoleRule>,
}

/// Traits every file of a role must have, and traits none may have. Traits are
/// written as `scanr roles` prints them, such as ``exported `layer` `` or
/// `imports .fake`.
#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleRule {
    #[serde(default)]
    pub require: Vec<String>,
    #[serde(default)]
    pub forbid: Vec<String>,
}

impl Config {
    /// Reads `explicit`, or `scanr.json` in `root`.
    pub fn load(root: &Path, explicit: Option<&str>) -> Result<Self> {
        let path = explicit.map_or_else(|| root.join(CONFIG_FILE), PathBuf::from);
        if !path.is_file() {
            bail!("no config at {}; create it with a `roles` section", path.display());
        }
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&source).with_context(|| format!("invalid config {}", path.display()))
    }
}

#[cfg(test)]
#[path = "config_test.rs"]
mod tests;
