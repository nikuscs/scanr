pub mod contextual_detectors;
pub mod diff;
pub mod exact_detectors;
pub mod facts;
pub mod low_value_detectors;
pub mod output;
pub mod test_detectors;
pub mod types;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::commands::dupes::function_duplicate_groups;
use crate::scan::typescript::parse::SimilarityFile;
use crate::slop::types::{
    AnalysisCoverage, CallRole, DeclarationMetadata, ImportKey, ImportResolution, OfflineProof,
    ProjectFacts, ReinventionMatch, Resolution, SlopOptions, SourceSpan, SymbolKey, TypeKey,
};

pub fn build_project_facts(root: &Path, files: Vec<SimilarityFile>) -> Result<ProjectFacts> {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut files = files;
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let function_groups = function_duplicate_groups(&files, 0.87, 3)?;
    let mut project = ProjectFacts {
        root: canonical_root.to_string_lossy().to_string(),
        entrypoints: discover_entrypoints(&canonical_root, &files),
        function_groups,
        coverage: AnalysisCoverage {
            unsupported_offline_proofs: BTreeSet::from([
                OfflineProof::CatchRecoveryIsFailure,
                OfflineProof::AsyncWithoutAwaitIsMisuse,
                OfflineProof::SequentialAwaitIsIndependent,
                OfflineProof::TypeResolvedMemberAbsence,
                OfflineProof::ExternalExportUnused,
                OfflineProof::PropertyOwnershipAndUsage,
                OfflineProof::ManifestDependencyUnused,
                OfflineProof::CustomAssertionExecution,
                OfflineProof::SemanticHelperInterchangeability,
                OfflineProof::RequestedScopeIntent,
            ]),
            ..AnalysisCoverage::default()
        },
        ..ProjectFacts::default()
    };

    for file in &files {
        if !file.slop.analysis_complete {
            project.coverage.parse_incomplete_files.insert(file.path.clone());
        }
        if !file.slop.dynamic_import_spans.is_empty() {
            project.coverage.unresolved_dynamic_imports.insert(file.path.clone());
        }
        if !file.slop.export_surface.unknown_star_reexports.is_empty() {
            project.coverage.unresolved_reexports.insert(file.path.clone());
        }
        for import in &file.slop.imports {
            if !import.source.starts_with('.') {
                project.coverage.external_consumer_unknown_packages.insert(import.source.clone());
            }
        }
        project.exports.insert(file.path.clone(), file.slop.export_surface.clone());
        index_declaration_metadata(file, &mut project.declaration_metadata);
        for reference in &file.slop.references {
            if let Resolution::Resolved(symbol) = &reference.resolved {
                project.symbol_uses.entry(symbol.clone()).or_default().push(reference.span.clone());
            }
        }
        for member in &file.slop.member_uses {
            if let Some(name) = &member.static_member {
                project.member_uses.entry(name.clone()).or_default().push(member.span.clone());
            }
        }
        project.files.insert(file.path.clone(), file.slop.clone());
    }

    normalize_span_map(&mut project.symbol_uses);
    normalize_span_map(&mut project.member_uses);
    project.type_groups = build_type_groups(&project);

    let aliases = load_tsconfig_aliases(&canonical_root, &mut project.coverage);
    resolve_imports(&canonical_root, &aliases, &mut project);
    index_import_uses(&mut project);
    index_call_targets(&mut project);
    index_shared_semantic_facts(&mut project);
    project.reinvention_matches = build_reinvention_matches(&project);
    Ok(project)
}

pub fn detectors(options: &SlopOptions) -> Vec<Box<dyn types::Detector>> {
    let mut registry = exact_detectors::detectors();
    let mut contextual = contextual_detectors::detectors(options);
    registry.extend(contextual.drain(..1));
    registry.extend(low_value_detectors::detectors(options));
    registry.extend(contextual);
    registry.extend(test_detectors::detectors());
    registry
}

fn index_declaration_metadata(
    file: &SimilarityFile,
    metadata: &mut BTreeMap<SymbolKey, DeclarationMetadata>,
) {
    use crate::slop::types::DeclarationKind;

    for declaration in &file.slop.declarations {
        let function = file.functions.iter().find(|function| {
            function.name == declaration.key.name
                && function.start_line == declaration.span.start_line
        });
        let is_function =
            matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Method);
        let top_level = if is_function {
            function.is_some_and(|function| {
                function.parent_function.is_none() && function.class_name.is_none()
            })
        } else {
            matches!(declaration.scope, crate::slop::types::ScopeKey::Module(_))
        };
        metadata.insert(
            declaration.key.clone(),
            DeclarationMetadata {
                top_level,
                parameter_count: function.map(|function| function.parameters.len()),
                capture_count: None,
                similarity_ignored: function.is_some_and(|function| function.has_ignore_directive),
                reference_count_is_exact: file.slop.analysis_complete,
            },
        );
    }
}

fn build_reinvention_matches(project: &ProjectFacts) -> Vec<ReinventionMatch> {
    let mut matches = Vec::new();
    for (first, component) in &project.function_groups {
        if component.first() != Some(first) {
            continue;
        }
        let declarations = component
            .iter()
            .filter_map(|(path, line, name)| {
                project.files.get(path)?.declarations.iter().find(|declaration| {
                    declaration.span.start_line == *line && declaration.key.name == *name
                })
            })
            .collect::<Vec<_>>();
        for candidate in &declarations {
            for existing in &declarations {
                if candidate.key != existing.key {
                    matches.push(ReinventionMatch {
                        candidate: candidate.key.clone(),
                        existing: existing.key.clone(),
                        similarity_millis: 870,
                    });
                }
            }
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

fn discover_entrypoints(root: &Path, files: &[SimilarityFile]) -> BTreeSet<String> {
    let mut package_dirs = BTreeSet::from([PathBuf::new()]);
    for file in files {
        let mut directory = Path::new(&file.path).parent();
        while let Some(path) = directory {
            package_dirs.insert(path.to_path_buf());
            directory = path.parent();
        }
    }
    package_dirs.retain(|directory| root.join(directory).join("package.json").is_file());

    let mut entrypoints = BTreeSet::new();
    for file in files {
        let path = Path::new(&file.path);
        let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
        let parent = path.parent().unwrap_or_else(|| Path::new(""));
        if matches!(stem, "index" | "main" | "mod")
            && (parent.as_os_str().is_empty() || package_dirs.contains(parent))
        {
            entrypoints.insert(file.path.clone());
        }
    }

    for directory in package_dirs {
        let manifest_path = root.join(&directory).join("package.json");
        let Ok(source) = fs::read_to_string(manifest_path) else { continue };
        let Ok(manifest) = serde_json::from_str::<Value>(&source) else { continue };
        let mut targets = Vec::new();
        for field in ["main", "module", "types"] {
            if let Some(target) = manifest.get(field).and_then(Value::as_str) {
                targets.push(target.to_string());
            }
        }
        if let Some(exports) = manifest.get("exports") {
            collect_manifest_targets(exports, &mut targets);
        }
        for target in targets {
            if target.contains('*') {
                continue;
            }
            let target = target.trim_start_matches("./");
            let path = directory.join(target).to_string_lossy().replace('\\', "/");
            entrypoints.insert(path);
        }
    }
    entrypoints
}

fn collect_manifest_targets(value: &Value, targets: &mut Vec<String>) {
    match value {
        Value::String(target) => targets.push(target.clone()),
        Value::Array(values) => {
            for value in values {
                collect_manifest_targets(value, targets);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_manifest_targets(value, targets);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn normalize_span_map<K: Ord>(map: &mut BTreeMap<K, Vec<SourceSpan>>) {
    for spans in map.values_mut() {
        spans.sort();
        spans.dedup();
    }
}

type ModelSignature = Vec<(String, bool, bool, Option<String>)>;

fn build_type_groups(project: &ProjectFacts) -> BTreeMap<TypeKey, Vec<TypeKey>> {
    let mut signatures: BTreeMap<ModelSignature, Vec<TypeKey>> = BTreeMap::new();
    for facts in project.files.values() {
        for model in &facts.models {
            let mut signature = model
                .fields
                .iter()
                .map(|field| {
                    (
                        field.name.to_ascii_lowercase(),
                        field.optional,
                        field.nullable,
                        field.primitive.clone(),
                    )
                })
                .collect::<Vec<_>>();
            signature.sort();
            signatures.entry(signature).or_default().push(model.key.clone());
        }
    }
    let mut groups = BTreeMap::new();
    for mut members in signatures.into_values().filter(|members| members.len() > 1) {
        members.sort();
        members.dedup();
        for member in &members {
            groups.insert(member.clone(), members.clone());
        }
    }
    groups
}

#[derive(Debug, Default)]
struct AliasConfig {
    base_url: PathBuf,
    paths: BTreeMap<String, Vec<String>>,
}

fn load_tsconfig_aliases(root: &Path, coverage: &mut AnalysisCoverage) -> AliasConfig {
    let path = root.join("tsconfig.json");
    let Ok(source) = fs::read_to_string(&path) else {
        return AliasConfig { base_url: root.to_path_buf(), ..AliasConfig::default() };
    };
    let Ok(json) = serde_json::from_str::<Value>(&source) else {
        coverage.unsupported_tsconfigs.insert(path.to_string_lossy().to_string());
        return AliasConfig { base_url: root.to_path_buf(), ..AliasConfig::default() };
    };
    let compiler = json.get("compilerOptions").and_then(Value::as_object);
    let base_url = compiler
        .and_then(|value| value.get("baseUrl"))
        .and_then(Value::as_str)
        .map_or_else(|| root.to_path_buf(), |value| root.join(value));
    let mut paths = BTreeMap::new();
    if let Some(entries) = compiler.and_then(|value| value.get("paths")).and_then(Value::as_object)
    {
        for (alias, targets) in entries {
            if let Some(targets) = targets.as_array() {
                paths.insert(
                    alias.clone(),
                    targets.iter().filter_map(Value::as_str).map(str::to_string).collect(),
                );
            }
        }
    }
    AliasConfig { base_url, paths }
}

fn resolve_imports(root: &Path, aliases: &AliasConfig, project: &mut ProjectFacts) {
    let file_paths = project.files.keys().cloned().collect::<BTreeSet<_>>();
    let mut resolutions = BTreeMap::new();
    for (importer, facts) in &project.files {
        for import in &facts.imports {
            let key = ImportKey {
                importer: importer.clone(),
                source: import.source.clone(),
                imported: import.imported.clone(),
                local: import.local.clone(),
                start_byte: import.span.start_byte,
            };
            let module = resolve_module(root, aliases, importer, &import.source, &file_paths);
            let export = resolve_export(project, &module, import.imported.as_deref());
            let resolved_symbol = match &export {
                Resolution::Resolved(symbol) => Some(symbol.clone()),
                _ => None,
            };
            resolutions.insert(key, ImportResolution { module, export, resolved_symbol });
        }
    }
    project.imports = resolutions;
}

fn resolve_module(
    root: &Path,
    aliases: &AliasConfig,
    importer: &str,
    source: &str,
    files: &BTreeSet<String>,
) -> Resolution<String> {
    let mut bases = Vec::new();
    if source.starts_with('.') {
        let parent = Path::new(importer).parent().unwrap_or_else(|| Path::new(""));
        bases.push(root.join(parent).join(source));
    } else {
        for (pattern, targets) in &aliases.paths {
            if let Some(capture) = alias_capture(pattern, source) {
                for target in targets {
                    bases.push(aliases.base_url.join(target.replace('*', capture)));
                }
            }
        }
        if bases.is_empty() {
            bases.extend(package_bases(root, source));
        }
    }

    let mut attempted = Vec::new();
    let mut candidates = Vec::new();
    for base in bases {
        for candidate in module_candidates(&base) {
            attempted.push(display_candidate(root, &candidate));
            if candidate.is_file() {
                candidates.push(display_candidate(root, &candidate));
            }
        }
    }
    attempted.sort();
    attempted.dedup();
    candidates.sort();
    candidates.dedup();

    let analyzed = candidates
        .iter()
        .filter(|candidate| files.contains(*candidate))
        .cloned()
        .collect::<Vec<_>>();
    match analyzed.as_slice() {
        [candidate] => Resolution::Resolved(candidate.clone()),
        [] if candidates.len() == 1 => Resolution::Resolved(candidates[0].clone()),
        [] if candidates.is_empty() => Resolution::Missing { attempted },
        [] => Resolution::Ambiguous { candidates },
        [..] => Resolution::Ambiguous { candidates: analyzed },
    }
}

fn resolve_export(
    project: &ProjectFacts,
    module: &Resolution<String>,
    imported: Option<&str>,
) -> Resolution<SymbolKey> {
    let Resolution::Resolved(path) = module else {
        return match module {
            Resolution::Missing { attempted } => {
                Resolution::Missing { attempted: attempted.clone() }
            }
            Resolution::Ambiguous { candidates } => Resolution::Unknown {
                reason: format!("module resolution is ambiguous: {}", candidates.join(", ")),
            },
            Resolution::Unknown { reason } => Resolution::Unknown { reason: reason.clone() },
            Resolution::Resolved(_) => unreachable!(),
        };
    };
    let Some(imported) = imported else {
        return Resolution::Unknown {
            reason: "side-effect import has no export symbol".to_string(),
        };
    };
    if imported == "*" {
        return Resolution::Unknown {
            reason: "namespace import resolves members individually".to_string(),
        };
    }
    let Some(surface) = project.exports.get(path) else {
        return Resolution::Unknown {
            reason: "resolved external module was not parsed; export surface is incomplete"
                .to_string(),
        };
    };
    let span = if imported == "default" {
        surface.default.as_ref().or_else(|| surface.names.get("default"))
    } else {
        surface.names.get(imported)
    };
    match span {
        Some(span) => {
            let symbol = project
                .files
                .get(path)
                .and_then(|facts| {
                    facts.declarations.iter().find(|declaration| {
                        declaration.key.name == imported
                            || declaration.exported_as.iter().any(|name| name == imported)
                    })
                })
                .map_or_else(
                    || SymbolKey {
                        path: path.clone(),
                        declaration_start: span.start_byte,
                        name: imported.to_string(),
                    },
                    |declaration| declaration.key.clone(),
                );
            Resolution::Resolved(symbol)
        }
        None if surface.complete => {
            Resolution::Missing { attempted: vec![format!("{path}#{imported}")] }
        }
        None => Resolution::Unknown { reason: "target export surface is incomplete".to_string() },
    }
}

fn index_import_uses(project: &mut ProjectFacts) {
    let mappings = project
        .imports
        .iter()
        .filter_map(|(key, resolution)| {
            let local = key.local.as_ref()?;
            let target = resolution.resolved_symbol.as_ref()?;
            let import = project.files.get(&key.importer)?.imports.iter().find(|import| {
                import.span.start_byte == key.start_byte && import.local.as_ref() == Some(local)
            })?;
            let local_key = project
                .symbol_uses
                .keys()
                .find(|symbol| {
                    symbol.path == key.importer
                        && symbol.name == *local
                        && (import.span.start_byte..=import.span.end_byte)
                            .contains(&symbol.declaration_start)
                })?
                .clone();
            Some((local_key, target.clone()))
        })
        .collect::<Vec<_>>();
    for (local, target) in mappings {
        if let Some(uses) = project.symbol_uses.remove(&local) {
            project.symbol_uses.entry(target).or_default().extend(uses);
        }
    }
    normalize_span_map(&mut project.symbol_uses);
}

fn index_call_targets(project: &mut ProjectFacts) {
    let declarations = project
        .files
        .values()
        .flat_map(|facts| facts.declarations.iter())
        .map(|declaration| declaration.key.clone())
        .collect::<Vec<_>>();
    let imports_by_local = project
        .imports
        .iter()
        .filter_map(|(key, resolution)| {
            key.local
                .as_ref()
                .zip(resolution.resolved_symbol.as_ref())
                .map(|(local, symbol)| ((key.importer.clone(), local.clone()), symbol.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    for facts in project.files.values_mut() {
        for call in &facts.calls {
            let target = call.callee_path.first().map_or_else(
                || Resolution::Unknown { reason: "dynamic callee".to_string() },
                |name| {
                    if let Some(symbol) = imports_by_local.get(&(facts.path.clone(), name.clone()))
                    {
                        return Resolution::Resolved(symbol.clone());
                    }
                    let mut candidates = declarations
                        .iter()
                        .filter(|declaration| {
                            declaration.path == facts.path && declaration.name == *name
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    candidates.sort();
                    candidates.dedup();
                    match candidates.as_slice() {
                        [symbol] => Resolution::Resolved(symbol.clone()),
                        [] => Resolution::Unknown {
                            reason: "callee is not a resolved local/imported symbol".to_string(),
                        },
                        _ => Resolution::Ambiguous { candidates },
                    }
                },
            );
            project.call_targets.insert(call.key.clone(), target.clone());
            if let Some(async_fact) = facts.async_calls.iter_mut().find(|fact| fact.key == call.key)
            {
                async_fact.callee_symbol = target;
            }
        }
    }
}

#[allow(clippy::too_many_lines)] // Builds all mutually-referencing shared indexes in one deterministic pass.
fn index_shared_semantic_facts(project: &mut ProjectFacts) {
    let symbol_remap = import_symbol_mappings(project);
    let call_targets = project.call_targets.clone();
    let non_test_paths = project
        .files
        .iter()
        .filter(|(_, file)| !file.is_test)
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();

    project.model_field_types.clear();
    project.symbol_types.clear();
    project.suites.clear();
    project.setups.clear();
    project.tests.clear();
    project.assertions.clear();
    project.mocks.clear();
    project.call_arguments.clear();
    project.call_roles.clear();
    project.production_expressions.clear();

    for file in project.files.values_mut() {
        for field_type in &file.model_field_types {
            project
                .model_field_types
                .insert((field_type.model.clone(), field_type.field.clone()), field_type.clone());
        }
        for symbol_type in file.symbol_types.values() {
            project.symbol_types.insert(symbol_type.key.clone(), symbol_type.clone());
        }
        for suite in &file.suites {
            project.suites.insert(suite.id.clone(), suite.clone());
        }
        for setup in &file.setups {
            project.setups.insert(setup.id.clone(), setup.clone());
        }
        for test in &file.test_cases {
            project.tests.insert(test.id.clone(), test.clone());
        }
        let assertion_starts =
            file.assertions.iter().map(|fact| fact.span.start_byte).collect::<BTreeSet<_>>();
        let mock_starts =
            file.mocks.iter().map(|fact| fact.span.start_byte).collect::<BTreeSet<_>>();
        let setup_spans = file
            .setups
            .iter()
            .filter_map(|setup| setup.callback_span.as_ref())
            .cloned()
            .collect::<Vec<_>>();
        for call in &file.calls {
            let role = if assertion_starts.contains(&call.key.start_byte) {
                CallRole::Assertion
            } else if mock_starts.contains(&call.key.start_byte) {
                CallRole::Mock
            } else if call.callee_path.first().is_some_and(|name| {
                matches!(
                    name.as_str(),
                    "test"
                        | "it"
                        | "specify"
                        | "describe"
                        | "suite"
                        | "beforeEach"
                        | "afterEach"
                        | "beforeAll"
                        | "afterAll"
                )
            }) {
                CallRole::TestFramework
            } else if setup_spans.iter().any(|span| {
                span.start_byte <= call.span.start_byte && span.end_byte >= call.span.end_byte
            }) {
                CallRole::Fixture
            } else if matches!(
                call_targets.get(&call.key),
                Some(Resolution::Resolved(symbol))
                    if symbol.path != file.path && non_test_paths.contains(&symbol.path)
            ) {
                CallRole::Sut
            } else {
                CallRole::Unknown
            };
            project.call_roles.insert(call.key.clone(), role);
        }
        for arguments in &mut file.call_arguments {
            for shape in &mut arguments.arguments {
                remap_expression_symbols(shape, &symbol_remap);
            }
            project.call_arguments.insert(arguments.call.clone(), arguments.clone());
        }
        for assertion in &mut file.assertions {
            if let Some(actual) = &mut assertion.actual {
                remap_expression_symbols(actual, &symbol_remap);
            }
            if let Some(expected) = &mut assertion.expected {
                remap_expression_symbols(expected, &symbol_remap);
            }
            assertion.invokes = assertion.invoked_call.as_ref().map_or_else(
                || Resolution::Unknown {
                    reason: "assertion actual is not one direct statically shaped call".to_string(),
                },
                |call| {
                    call_targets.get(call).cloned().unwrap_or_else(|| Resolution::Unknown {
                        reason: "assertion call target is absent from the project index"
                            .to_string(),
                    })
                },
            );
            project.assertions.insert(assertion.span.clone(), assertion.clone());
        }
        for mock in &file.mocks {
            project.mocks.insert(mock.span.clone(), mock.clone());
        }
        for production in &mut file.production_expressions {
            remap_expression_symbols(&mut production.returned, &symbol_remap);
            project.production_expressions.insert(production.owner.clone(), production.clone());
        }
    }
}

fn import_symbol_mappings(project: &ProjectFacts) -> BTreeMap<SymbolKey, SymbolKey> {
    project
        .imports
        .iter()
        .filter_map(|(key, resolution)| {
            let local = key.local.as_ref()?;
            let target = resolution.resolved_symbol.as_ref()?;
            let import = project.files.get(&key.importer)?.imports.iter().find(|import| {
                import.span.start_byte == key.start_byte && import.local.as_ref() == Some(local)
            })?;
            let local_symbol =
                project.files.get(&key.importer)?.references.iter().find_map(|reference| {
                    match &reference.resolved {
                        Resolution::Resolved(symbol)
                            if symbol.name == *local
                                && (import.span.start_byte..=import.span.end_byte)
                                    .contains(&symbol.declaration_start) =>
                        {
                            Some(symbol.clone())
                        }
                        _ => None,
                    }
                })?;
            Some((local_symbol, target.clone()))
        })
        .collect()
}

fn remap_expression_symbols(
    shape: &mut crate::slop::types::ExpressionShape,
    mappings: &BTreeMap<SymbolKey, SymbolKey>,
) {
    for symbol in &mut shape.referenced_symbols {
        if let Some(target) = mappings.get(symbol) {
            *symbol = target.clone();
        }
    }
    shape.referenced_symbols.sort();
    shape.referenced_symbols.dedup();
}

fn alias_capture<'a>(pattern: &str, source: &'a str) -> Option<&'a str> {
    let (prefix, suffix) = pattern.split_once('*')?;
    source.strip_prefix(prefix)?.strip_suffix(suffix)
}

fn package_bases(root: &Path, source: &str) -> Vec<PathBuf> {
    let package_name = if source.starts_with('@') {
        source.split('/').take(2).collect::<Vec<_>>().join("/")
    } else {
        source.split('/').next().unwrap_or(source).to_string()
    };
    let package_root = root.join("node_modules").join(&package_name);
    let subpath = source.strip_prefix(&package_name).unwrap_or_default().trim_start_matches('/');
    if !subpath.is_empty() {
        return vec![package_root.join(subpath)];
    }
    let mut bases = vec![package_root.clone()];
    if let Ok(manifest) = fs::read_to_string(package_root.join("package.json"))
        && let Ok(json) = serde_json::from_str::<Value>(&manifest)
    {
        for field in ["types", "typings", "module", "main"] {
            if let Some(target) = json.get(field).and_then(Value::as_str) {
                bases.push(package_root.join(target));
            }
        }
    }
    bases
}

fn module_candidates(base: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![base.to_path_buf()];
    if base.extension().is_none() {
        for extension in ["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs", "d.ts"] {
            candidates.push(base.with_extension(extension));
        }
        for name in ["index.ts", "index.tsx", "index.js", "index.jsx", "index.d.ts"] {
            candidates.push(base.join(name));
        }
    }
    candidates
}

fn display_candidate(root: &Path, candidate: &Path) -> String {
    let normalized = candidate.canonicalize().unwrap_or_else(|_| candidate.to_path_buf());
    normalized.strip_prefix(root).unwrap_or(candidate).to_string_lossy().replace('\\', "/")
}

pub fn collect_project_files(root: &Path) -> Result<Vec<SimilarityFile>> {
    use rayon::prelude::*;

    use crate::scan::types::FunctionKindsFilter;
    use crate::scan::typescript::parse::process_file_with_similarity;
    use crate::scan::{ScanConfig, collect_files};

    let paths = collect_files(root, &ScanConfig::default())?;
    paths
        .par_iter()
        .map(|path| {
            process_file_with_similarity(path, root, FunctionKindsFilter::All)
                .map(|(_, similarity)| similarity)
                .with_context(|| format!("failed to collect facts from {}", path.display()))
        })
        .collect()
}
