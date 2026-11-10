//! File-level dependency graph: each file's resolved imports and its importers.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::slop::types::{ProjectFacts, Resolution};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    /// Module specifier as written.
    pub source: String,
    /// Project file the specifier resolves to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Package name when the specifier leaves the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Every import of this module is `import type`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub type_only: bool,
}

impl Dependency {
    /// Neither a project file nor a package: a relative path that leads nowhere,
    /// or one that matches several files.
    pub const fn is_unresolved(&self) -> bool {
        self.file.is_none() && self.package.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDependencies {
    pub imports: Vec<Dependency>,
    pub imported_by: BTreeSet<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub files: BTreeMap<String, FileDependencies>,
}

impl DependencyGraph {
    pub fn build(project: &ProjectFacts) -> Self {
        let mut files: BTreeMap<String, FileDependencies> =
            project.files.keys().map(|path| (path.clone(), FileDependencies::default())).collect();

        for (importer, facts) in &project.files {
            let resolutions = project.module_resolutions.get(importer);
            for source in &facts.module_sources {
                let resolution = resolutions.and_then(|modules| modules.get(source));
                let (file, package) = target(source, resolution);
                let mut imports = facts.imports.iter().filter(|import| import.source == *source);
                let type_only = imports.clone().next().is_some() && imports.all(|i| i.type_only);

                if let Some(file) = &file
                    && file != importer
                    && let Some(target) = files.get_mut(file)
                {
                    target.imported_by.insert(importer.clone());
                }
                if let Some(entry) = files.get_mut(importer) {
                    entry.imports.push(Dependency {
                        source: source.clone(),
                        file,
                        package,
                        type_only,
                    });
                }
            }
        }
        Self { files }
    }
}

/// Splits a resolution into a project file or a package name.
fn target(
    source: &str,
    resolution: Option<&Resolution<String>>,
) -> (Option<String>, Option<String>) {
    let relative = source.starts_with('.') || source.starts_with('/');
    match resolution {
        Some(Resolution::Resolved(path)) if !path.split('/').any(|part| part == "node_modules") => {
            (Some(path.clone()), None)
        }
        _ if relative => (None, None),
        _ => (None, Some(package_name(source))),
    }
}

/// `@scope/name/sub` → `@scope/name`, `effect/Effect` → `effect`, `node:fs` stays.
fn package_name(source: &str) -> String {
    let segments = if source.starts_with('@') { 2 } else { 1 };
    source.split('/').take(segments).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
#[path = "dependencies_test.rs"]
mod tests;
