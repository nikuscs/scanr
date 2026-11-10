//! `scanr roles --check`: holds every file to the rules its role has in `scanr.json`.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::Serialize;

use super::RoleFile;
use crate::config::RoleRule;
use crate::scan::outline::SymbolKind;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Violation {
    pub file: String,
    pub role: String,
    /// Required traits the file does not have.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// Forbidden traits the file has.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forbidden: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub version: u8,
    /// Files held to a rule.
    pub checked: usize,
    pub violations: Vec<Violation>,
    /// Roles with a rule and no file: a renamed suffix, or a typo in the config.
    pub unused_rules: Vec<String>,
}

/// Fails on a trait `scanr roles` could never print, so a typo in `forbid`
/// cannot pass as a rule that is always satisfied.
pub fn validate_rules(rules: &BTreeMap<String, RoleRule>) -> Result<()> {
    for (role, rule) in rules {
        for name in rule.require.iter().chain(&rule.forbid) {
            if !is_trait(name) {
                bail!(
                    "role `{role}`: `{name}` is not a trait; copy one from `scanr roles`, such as \
                     \"exported `layer`\", \"imports .fake\", \"internal class\", or \"re-exports\""
                );
            }
        }
    }
    Ok(())
}

/// Whether `name` has the shape of a trait `file_traits` produces.
fn is_trait(name: &str) -> bool {
    let quoted = |name: &str| name.len() > 2 && name.starts_with('`') && name.ends_with('`');
    if name == "re-exports" || quoted(name) {
        return true;
    }
    if let Some(role) = name.strip_prefix("imports ") {
        return !role.is_empty();
    }
    let Some(rest) = name.strip_prefix("exported ").or_else(|| name.strip_prefix("internal "))
    else {
        return false;
    };
    if name.starts_with("exported ") && quoted(rest) {
        return true;
    }
    let (kind, producer) = rest.split_once(" = ").map_or((rest, None), |(k, p)| (k, Some(p)));
    SymbolKind::ALL.iter().any(|known| known.as_str() == kind)
        && producer.is_none_or(|producer| !producer.is_empty())
}

pub fn check(files: &[RoleFile], rules: &BTreeMap<String, RoleRule>) -> CheckReport {
    let mut checked = 0;
    let mut violations = Vec::new();
    for file in files {
        let Some(rule) = rules.get(&file.role) else {
            continue;
        };
        checked += 1;
        let has = |name: &String| file.traits.contains_key(name);
        let missing: Vec<String> = rule.require.iter().filter(|name| !has(name)).cloned().collect();
        let forbidden: Vec<String> = rule.forbid.iter().filter(|name| has(name)).cloned().collect();
        if !missing.is_empty() || !forbidden.is_empty() {
            violations.push(Violation {
                file: file.path.clone(),
                role: file.role.clone(),
                missing,
                forbidden,
            });
        }
    }
    violations.sort_by(|a, b| (&a.role, &a.file).cmp(&(&b.role, &b.file)));
    let unused_rules = rules
        .keys()
        .filter(|role| !files.iter().any(|file| file.role == **role))
        .cloned()
        .collect();
    CheckReport { version: 1, checked, violations, unused_rules }
}

pub fn render(report: &CheckReport) -> String {
    let mut out = String::from("# Role rules\n");
    let mut role = "";
    for violation in &report.violations {
        if violation.role != role {
            role = &violation.role;
            out.push_str(&format!("\n## {role}\n"));
        }
        out.push_str(&format!("- {}\n", violation.file));
        if !violation.missing.is_empty() {
            out.push_str(&format!("    missing    {}\n", violation.missing.join(", ")));
        }
        if !violation.forbidden.is_empty() {
            out.push_str(&format!("    forbidden  {}\n", violation.forbidden.join(", ")));
        }
    }
    if !report.unused_rules.is_empty() {
        out.push_str(&format!(
            "\nRules for roles with no files: {}\n",
            report.unused_rules.join(", ")
        ));
    }
    out.push_str(&format!(
        "\nFiles breaking a rule: {} of {} checked.\n",
        report.violations.len(),
        report.checked
    ));
    out
}

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
