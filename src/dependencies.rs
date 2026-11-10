//! File-level dependency graph: each file's resolved imports and its importers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::cycles::{Graph, find_cycles};
use crate::scan::rules::is_test_path;
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

/// Import loops in shipped code. Type-only imports are erased at build time and
/// test files are not shipped, so neither can form a loop.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Cycles {
    pub files: Vec<Vec<String>>,
    pub packages: Vec<Vec<String>>,
}

impl Cycles {
    pub const fn is_empty(&self) -> bool {
        self.files.is_empty() && self.packages.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub files: BTreeMap<String, FileDependencies>,
    /// Workspace packages: directory of each `package.json` under the root, by package name.
    packages: BTreeMap<String, String>,
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
        Self { packages: workspace_packages(Path::new(&project.root), files.keys()), files }
    }

    pub fn cycles(&self) -> Cycles {
        let mut file_graph = Graph::new();
        let mut package_graph = Graph::new();
        for (importer, dependencies) in self.files.iter().filter(|(file, _)| !is_test_path(file)) {
            let from_package = self.package_of(importer);
            let targets = file_graph.entry(importer.clone()).or_default();
            for import in dependencies.imports.iter().filter(|import| !import.type_only) {
                if let Some(file) =
                    import.file.as_ref().filter(|file| self.files.contains_key(*file))
                {
                    targets.insert(file.clone());
                }
                let to_package = match (&import.file, &import.package) {
                    (Some(file), _) => self.package_of(file),
                    (None, Some(name)) => self.packages.contains_key(name).then_some(name.as_str()),
                    (None, None) => None,
                };
                if let (Some(from), Some(to)) = (from_package, to_package)
                    && from != to
                {
                    package_graph.entry(from.to_string()).or_default().insert(to.to_string());
                }
            }
        }
        Cycles { files: find_cycles(&file_graph), packages: find_cycles(&package_graph) }
    }

    /// Name of the workspace package whose directory most closely contains `file`.
    fn package_of(&self, file: &str) -> Option<&str> {
        self.packages
            .iter()
            .filter(|(_, directory)| {
                directory.is_empty()
                    || file
                        .strip_prefix(directory.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|(_, directory)| directory.len())
            .map(|(name, _)| name.as_str())
    }
}

/// Reads the `name` of every `package.json` in a directory that holds analyzed files.
fn workspace_packages<'a>(
    root: &Path,
    files: impl Iterator<Item = &'a String>,
) -> BTreeMap<String, String> {
    let directories: BTreeSet<&Path> =
        files.flat_map(|file| Path::new(file.as_str()).ancestors().skip(1)).collect();
    directories
        .into_iter()
        .filter_map(|directory| {
            let manifest =
                std::fs::read_to_string(root.join(directory).join("package.json")).ok()?;
            let json: serde_json::Value = serde_json::from_str(&manifest).ok()?;
            let name = json.get("name")?.as_str()?.to_string();
            Some((name, directory.to_string_lossy().replace('\\', "/")))
        })
        .collect()
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
