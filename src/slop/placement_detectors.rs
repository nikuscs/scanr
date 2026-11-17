//! Cross-file placement signals: a type declared twice, and an export only one
//! other file uses. A per-file linter cannot see either.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::scan::types::FunctionRole;
use crate::slop::types::{
    DeclarationFact, DeclarationKind, Detector, FileFacts, ModelFact, ModuleShape, ProjectFacts,
    SlopConfidence, SlopEvidence, SlopFinding, SlopKind, SourceSpan,
};

/// Fewest fields a model needs before an identical copy is worth reporting.
const MIN_COPIED_FIELDS: usize = 3;
/// Shortest class name compared across files.
const MIN_CLASS_NAME: usize = 4;

pub fn detectors() -> Vec<Box<dyn Detector>> {
    vec![Box::new(CopiedType), Box::new(SingleUseExport)]
}

const fn in_scope(file: &FileFacts) -> bool {
    file.analysis_complete && !file.is_test && !file.is_generated
}

fn uses(project: &ProjectFacts, declaration: &DeclarationFact) -> usize {
    project.symbol_uses.get(&declaration.key).map_or(0, Vec::len)
}

fn row(code: &str, label: &str, span: &SourceSpan, detail: String) -> SlopEvidence {
    SlopEvidence { code: code.to_string(), label: label.to_string(), span: span.clone(), detail }
}

struct CopiedType;

impl Detector for CopiedType {
    fn kind(&self) -> SlopKind {
        SlopKind::CopiedType
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = copied_models(project);
        findings.extend(copied_classes(project));
        findings.sort_by(|left, right| left.span.cmp(&right.span));
        findings
    }
}

/// Interfaces and object type aliases whose bodies are the same code in more than
/// one file.
fn copied_models(project: &ProjectFacts) -> Vec<SlopFinding> {
    let mut groups: BTreeMap<u64, Vec<&ModelFact>> = BTreeMap::new();
    for file in project.files.values().filter(|file| in_scope(file)) {
        for model in &file.models {
            if model.fields.len() < MIN_COPIED_FIELDS || !model.extends.is_empty() {
                continue;
            }
            if let Some(fingerprint) = file.model_fingerprints.get(&model.key.declaration_start) {
                groups.entry(*fingerprint).or_default().push(model);
            }
        }
    }
    groups
        .into_values()
        .filter(|models| {
            models.iter().map(|model| &model.key.path).collect::<BTreeSet<_>>().len() > 1
                && models.iter().any(|model| model.exported)
        })
        .map(|mut models| {
            // The copy is the model fewer files lean on; the most-used one is the original.
            models.sort_by_key(|model| {
                let used = project
                    .files
                    .get(&model.key.path)
                    .and_then(|file| file.declarations.iter().find(|d| d.key.name == model.key.name))
                    .map_or(0, |declaration| uses(project, declaration));
                (used, model.exported, std::cmp::Reverse(model.key.path.clone()))
            });
            let fields = models[0].fields.len();
            let rows = models
                .iter()
                .enumerate()
                .map(|(index, model)| {
                    let label = if index == 0 { "copy" } else { "same shape" };
                    row(&model.key.name, label, &model.span, format!("{fields} fields, identical code"))
                })
                .collect();
            finding(
                rows,
                "Several files declare the same type, with identical code.",
                "Keep the type in the module that owns it (usually the contract) and import it everywhere else.",
            )
        })
        .collect()
}

/// Classes, usually error classes, declared with the same name and the same code in
/// more than one file. Same-named classes with different code (per-package service
/// tags) are a convention, not a copy.
fn copied_classes(project: &ProjectFacts) -> Vec<SlopFinding> {
    let mut groups: BTreeMap<(&str, u64), Vec<&DeclarationFact>> = BTreeMap::new();
    for file in project.files.values().filter(|file| in_scope(file)) {
        for declaration in &file.declarations {
            let Some(fingerprint) = file.class_fingerprints.get(&declaration.key.declaration_start)
            else {
                continue;
            };
            if declaration.kind == DeclarationKind::Class
                && !declaration.ambient
                && declaration.key.name.len() >= MIN_CLASS_NAME
                && is_top_level(project, declaration)
            {
                groups.entry((&declaration.key.name, *fingerprint)).or_default().push(declaration);
            }
        }
    }
    groups
        .into_values()
        .filter(|classes| {
            classes.iter().map(|class| &class.key.path).collect::<BTreeSet<_>>().len() > 1
                && classes.iter().any(|class| !class.exported_as.is_empty())
        })
        .map(|mut classes| {
            classes.sort_by_key(|class| {
                (uses(project, class), std::cmp::Reverse(class.key.path.clone()))
            });
            let rows = classes
                .iter()
                .enumerate()
                .map(|(index, class)| {
                    let label = if index == 0 { "copy" } else { "same code" };
                    let detail = format!("class `{}`, {} uses", class.key.name, uses(project, class));
                    row(&class.key.name, label, &class.span, detail)
                })
                .collect();
            finding(
                rows,
                "Several files declare the same class, with identical code.",
                "Keep one class in the module that owns it (errors usually live in the contract) and import it everywhere else.",
            )
        })
        .collect()
}

struct SingleUseExport;

impl Detector for SingleUseExport {
    fn kind(&self) -> SlopKind {
        SlopKind::SingleUseExport
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let root = Path::new(&project.root);
        let mut packages = BTreeMap::new();
        let mut findings = Vec::new();
        for file in project.files.values() {
            if !in_scope(file)
                || project.entrypoints.contains(&file.path)
                || file.module_shape == ModuleShape::PassThrough
            {
                continue;
            }
            // A file with one importer is `oversplit-module`'s case; this one is about a
            // shared file carrying an export that only one of its importers needs.
            let importers = production_importers(project, &file.path);
            // A file with no functions (constants, types, config) is a deliberate home
            // for values, even ones a single file reads.
            let has_functions = file.declarations.iter().any(|declaration| {
                declaration.kind == DeclarationKind::Function
                    && !declaration.key.name.starts_with('<')
                    && is_top_level(project, declaration)
            });
            if importers.len() < 2 || !has_functions {
                continue;
            }
            for declaration in &file.declarations {
                let Some(consumer) = single_consumer(project, file, declaration) else {
                    continue;
                };
                // Component files hold components only, so a component consumer is not
                // where the value should move.
                if is_component_file(&consumer.path)
                    || re_exported(project, &importers, declaration)
                    || package_of(root, &file.path, &mut packages)
                        != package_of(root, &consumer.path, &mut packages)
                {
                    continue;
                }
                let name = &declaration.key.name;
                findings.push(SlopFinding {
                    kind: SlopKind::SingleUseExport,
                    confidence: SlopConfidence::Medium,
                    span: declaration.span.clone(),
                    evidence: vec![
                        row(name, "export", &declaration.span, format!("`{name}` is exported")),
                        row(
                            name,
                            "only consumer",
                            &consumer,
                            format!("the only file that uses `{name}`"),
                        ),
                    ],
                    explanation: format!(
                        "`{name}` is exported, but only `{}` uses it.",
                        consumer.path
                    ),
                    action: format!(
                        "Move `{name}` into `{}` and drop the export, or inline it there.",
                        consumer.path
                    ),
                });
            }
        }
        findings.sort_by(|left, right| left.span.cmp(&right.span));
        findings
    }
}

/// First use of an exported helper or literal constant when exactly one other
/// production file uses it and its own file does not.
fn single_consumer(
    project: &ProjectFacts,
    file: &FileFacts,
    declaration: &DeclarationFact,
) -> Option<SourceSpan> {
    let candidate = match declaration.kind {
        DeclarationKind::Function => declaration.role == Some(FunctionRole::Helper),
        DeclarationKind::Variable => {
            file.literal_constants.contains(&declaration.key.declaration_start)
        }
        _ => false,
    };
    if !candidate
        || declaration.exported_as.is_empty()
        || declaration.exported_as.iter().any(|name| name == "default")
        || !project
            .declaration_metadata
            .get(&declaration.key)
            .is_some_and(|metadata| metadata.top_level && metadata.reference_count_is_exact)
    {
        return None;
    }
    let spans = project.symbol_uses.get(&declaration.key)?;
    let mut consumers = BTreeSet::new();
    let mut first = None;
    for span in spans {
        if span.path == file.path {
            return None;
        }
        if project.files.get(&span.path).is_some_and(|user| user.is_test) {
            continue;
        }
        consumers.insert(&span.path);
        first.get_or_insert(span);
    }
    (consumers.len() == 1).then(|| first.cloned()).flatten()
}

fn production_importers(project: &ProjectFacts, path: &str) -> BTreeSet<String> {
    project
        .module_importers
        .get(path)
        .into_iter()
        .flatten()
        .filter(|importer| project.files.get(*importer).is_some_and(|file| !file.is_test))
        .cloned()
        .collect()
}

/// An importer that exports the same name forwards it as public surface.
fn re_exported(
    project: &ProjectFacts,
    importers: &BTreeSet<String>,
    declaration: &DeclarationFact,
) -> bool {
    importers.iter().any(|importer| {
        project.exports.get(importer).is_some_and(|surface| {
            declaration.exported_as.iter().any(|name| surface.names.contains_key(name))
        })
    })
}

/// Directory of the nearest `package.json` above a root-relative path.
fn package_of(root: &Path, path: &str, cache: &mut BTreeMap<String, String>) -> String {
    let mut directory = Path::new(path).parent();
    while let Some(current) = directory {
        let key = current.to_string_lossy().to_string();
        if let Some(found) = cache.get(&key) {
            return found.clone();
        }
        if root.join(current).join("package.json").is_file() {
            cache.insert(key.clone(), key.clone());
            return key;
        }
        directory = current.parent();
    }
    String::new()
}

fn is_component_file(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|extension| extension == "tsx" || extension == "jsx")
}

fn is_top_level(project: &ProjectFacts, declaration: &DeclarationFact) -> bool {
    project.declaration_metadata.get(&declaration.key).is_some_and(|metadata| metadata.top_level)
}

fn finding(rows: Vec<SlopEvidence>, explanation: &str, action: &str) -> SlopFinding {
    SlopFinding {
        kind: SlopKind::CopiedType,
        confidence: SlopConfidence::Medium,
        span: rows[0].span.clone(),
        evidence: rows,
        explanation: explanation.to_string(),
        action: action.to_string(),
    }
}
