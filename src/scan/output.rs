use std::io::Write;

use serde::Serialize;

use super::types::{BindingKind, FunctionInfo, FunctionKind, OutputMode, ScanResult, Stats};

pub fn write_result_with_lines<W: Write>(
    result: &ScanResult,
    mode: OutputMode,
    include_lines: bool,
    w: &mut W,
) -> anyhow::Result<()> {
    match mode {
        OutputMode::Compact => {
            let compact = CompactOutput::from(result);
            serde_json::to_writer(w, &compact)?;
        }
        OutputMode::Verbose => {
            let verbose = VerboseOutput::from(result);
            serde_json::to_writer_pretty(w, &verbose)?;
        }
        OutputMode::Files => {
            let files = FilesOutput::from(result);
            serde_json::to_writer_pretty(w, &files)?;
        }
        OutputMode::Folders => {
            let folders = FoldersOutput::from(result);
            serde_json::to_writer_pretty(w, &folders)?;
        }
        OutputMode::Inventory => {
            let inventory = InventoryOutput::from_result(result, include_lines);
            serde_json::to_writer(w, &inventory)?;
        }
    }
    Ok(())
}

pub fn write_schema<W: Write>(mode: OutputMode, w: &mut W) -> anyhow::Result<()> {
    let schema = match mode {
        OutputMode::Compact => serde_json::json!({
            "mode": "compact",
            "ver": 1,
            "fields": {
                "f": ["file", "line", "col", "name", "exported", "kind", "parent", "captures", "low_value_reason"],
                "b": ["file", "line", "col", "name", "kind", "refs"],
                "t": ["file", "line", "col", "name", "exported", "kind"],
                "x": ["file", "name", "kind_code"],
                "viol": ["file", "rule", "count", "details"],
                "err": ["message"]
            },
            "export_kind_codes": { "1": "named", "2": "default", "3": "reexport" },
            "type_kinds": ["interface", "type"]
        }),
        OutputMode::Verbose => serde_json::json!({
            "mode": "verbose",
            "ver": 1,
            "fields": {
                "functions": ["file", "name", "parent", "captures", "lowValueReason", "role", "kind", "exported", "isAsync", "isGenerator", "span"],
                "bindings": ["file", "name", "kind", "exported", "refs", "decl"],
                "types": ["file", "name", "kind", "exported", "decl"],
                "exports": ["file", "name", "kindCode"],
                "violations": ["file", "rule", "count", "details"],
                "errors": ["message"]
            },
            "export_kind_codes": { "1": "named", "2": "default", "3": "reexport" },
            "type_kinds": ["interface", "type"]
        }),
        OutputMode::Files => serde_json::json!({
            "mode": "files",
            "ver": 1,
            "fields": {
                "files": "{ path: [dottedFunctionName, ...] }"
            }
        }),
        OutputMode::Folders => serde_json::json!({
            "mode": "folders",
            "ver": 1,
            "fields": {
                "folders": "{ dir: { functions, names } }"
            }
        }),
        OutputMode::Inventory => serde_json::json!({
            "mode": "inventory",
            "ver": 1,
            "fields": {
                "functions": ["file", "name", "line?"],
                "constants": ["file", "name", "kind", "line?"],
                "types": ["file", "name", "kind", "line?"],
                "components": ["file", "name", "line?"],
                "hooks": ["file", "name", "line?"],
                "classes": ["file", "name", "line?"],
                "enums": ["file", "name", "line?"],
                "exports": ["file", "name"]
            },
            "constant_kinds": ["primitive", "arrow"],
            "type_kinds": ["interface", "type"]
        }),
    };
    serde_json::to_writer(w, &schema)?;
    Ok(())
}

type CompactFunction =
    (String, u32, u32, String, u8, String, Option<String>, Vec<String>, Option<String>);

/// Types: [file, line, col, name, exported(0/1), kind]
type CompactType = (String, u32, u32, String, u8, String);

#[derive(Serialize)]
struct CompactOutput {
    ver: u8,
    stats: Stats,
    /// Functions: [file, line, col, name, exported(0/1), kind, parent, captures, `low_value_reason`]
    f: Vec<CompactFunction>,
    /// Bindings: [file, line, col, name, kind, refs]
    b: Vec<(String, u32, u32, String, String, usize)>,
    /// Types: [file, line, col, name, exported(0/1), kind]
    t: Vec<CompactType>,
    /// Exports: `[file, name, kind_code]`
    x: Vec<(String, String, u8)>,
    /// Violations: [file, rule, count, details]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    viol: Vec<(String, String, usize, Vec<String>)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    err: Vec<String>,
}

impl From<&ScanResult> for CompactOutput {
    fn from(r: &ScanResult) -> Self {
        let total_functions = r.file_indices.iter().map(|fi| fi.functions.len()).sum();
        let total_bindings = r.file_indices.iter().map(|fi| fi.bindings.len()).sum();
        let total_types = r.file_indices.iter().map(|fi| fi.types.len()).sum();
        let total_exports = r.file_indices.iter().map(|fi| fi.exports.len()).sum();
        let total_violations = r.file_indices.iter().map(|fi| fi.violations.len()).sum();
        let mut functions = Vec::with_capacity(total_functions);
        let mut bindings = Vec::with_capacity(total_bindings);
        let mut types = Vec::with_capacity(total_types);
        let mut exports = Vec::with_capacity(total_exports);
        let mut viol = Vec::with_capacity(total_violations);

        for fi in &r.file_indices {
            for func in &fi.functions {
                functions.push((
                    fi.path.clone(),
                    func.line,
                    func.col,
                    func.name.clone().unwrap_or_default(),
                    u8::from(func.exported),
                    func.kind.code().to_string(),
                    func.parent.clone(),
                    func.captures.clone(),
                    func.low_value_reason.clone(),
                ));
            }
            for binding in &fi.bindings {
                bindings.push((
                    fi.path.clone(),
                    binding.line,
                    binding.col,
                    binding.name.clone(),
                    binding.kind.as_str().to_string(),
                    binding.refs,
                ));
            }
            for type_info in &fi.types {
                types.push((
                    fi.path.clone(),
                    type_info.line,
                    type_info.col,
                    type_info.name.clone(),
                    u8::from(type_info.exported),
                    type_info.kind.as_str().to_string(),
                ));
            }
            for exp in &fi.exports {
                exports.push((fi.path.clone(), exp.name.clone(), exp.kind_code));
            }
            for violation in &fi.violations {
                viol.push((
                    fi.path.clone(),
                    violation.rule.clone(),
                    violation.count,
                    violation.details.clone(),
                ));
            }
        }

        Self {
            ver: r.ver,
            stats: r.stats.clone(),
            f: functions,
            b: bindings,
            t: types,
            x: exports,
            viol,
            err: r.errors.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerboseOutput {
    ver: u8,
    root: String,
    stats: Stats,
    functions: Vec<VerboseFunction>,
    bindings: Vec<VerboseBinding>,
    types: Vec<VerboseType>,
    exports: Vec<VerboseExport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    violations: Vec<VerboseViolation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerboseFunction {
    file: String,
    name: Option<String>,
    parent: Option<String>,
    captures: Vec<String>,
    low_value_reason: Option<String>,
    role: String,
    kind: String,
    exported: bool,
    is_async: bool,
    is_generator: bool,
    span: VerboseSpan,
}

#[derive(Serialize)]
struct VerboseSpan {
    start: VerbosePos,
    end: VerbosePos,
}

#[derive(Serialize)]
struct VerbosePos {
    line: u32,
    col: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerboseBinding {
    file: String,
    name: String,
    kind: String,
    exported: bool,
    refs: usize,
    decl: VerbosePos,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerboseType {
    file: String,
    name: String,
    kind: String,
    exported: bool,
    decl: VerbosePos,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerboseExport {
    file: String,
    name: String,
    kind_code: u8,
}

#[derive(Serialize)]
struct VerboseViolation {
    file: String,
    rule: String,
    count: usize,
    details: Vec<String>,
}

impl From<&ScanResult> for VerboseOutput {
    fn from(r: &ScanResult) -> Self {
        let total_functions = r.file_indices.iter().map(|fi| fi.functions.len()).sum();
        let total_bindings = r.file_indices.iter().map(|fi| fi.bindings.len()).sum();
        let total_types = r.file_indices.iter().map(|fi| fi.types.len()).sum();
        let total_exports = r.file_indices.iter().map(|fi| fi.exports.len()).sum();
        let total_violations = r.file_indices.iter().map(|fi| fi.violations.len()).sum();
        let mut functions = Vec::with_capacity(total_functions);
        let mut bindings = Vec::with_capacity(total_bindings);
        let mut types = Vec::with_capacity(total_types);
        let mut exports = Vec::with_capacity(total_exports);
        let mut violations = Vec::with_capacity(total_violations);

        for fi in &r.file_indices {
            for func in &fi.functions {
                functions.push(VerboseFunction {
                    file: fi.path.clone(),
                    name: func.name.clone(),
                    parent: func.parent.clone(),
                    captures: func.captures.clone(),
                    low_value_reason: func.low_value_reason.clone(),
                    role: func.role(&fi.path).as_str().to_string(),
                    kind: func.kind.label().to_string(),
                    exported: func.exported,
                    is_async: func.is_async,
                    is_generator: func.is_generator,
                    span: VerboseSpan {
                        start: VerbosePos { line: func.line, col: func.col },
                        end: VerbosePos { line: func.line_end, col: 0 },
                    },
                });
            }
            for binding in &fi.bindings {
                bindings.push(VerboseBinding {
                    file: fi.path.clone(),
                    name: binding.name.clone(),
                    kind: binding.kind.as_str().to_string(),
                    exported: binding.exported,
                    refs: binding.refs,
                    decl: VerbosePos { line: binding.line, col: binding.col },
                });
            }
            for type_info in &fi.types {
                types.push(VerboseType {
                    file: fi.path.clone(),
                    name: type_info.name.clone(),
                    kind: type_info.kind.as_str().to_string(),
                    exported: type_info.exported,
                    decl: VerbosePos { line: type_info.line, col: type_info.col },
                });
            }
            for exp in &fi.exports {
                exports.push(VerboseExport {
                    file: fi.path.clone(),
                    name: exp.name.clone(),
                    kind_code: exp.kind_code,
                });
            }
            for violation in &fi.violations {
                violations.push(VerboseViolation {
                    file: fi.path.clone(),
                    rule: violation.rule.clone(),
                    count: violation.count,
                    details: violation.details.clone(),
                });
            }
        }

        Self {
            ver: r.ver,
            root: r.root.clone(),
            stats: r.stats.clone(),
            functions,
            bindings,
            types,
            exports,
            violations,
            errors: r.errors.clone(),
        }
    }
}

use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FilesOutput {
    ver: u8,
    stats: Stats,
    files: BTreeMap<String, Vec<String>>,
}

impl From<&ScanResult> for FilesOutput {
    fn from(r: &ScanResult) -> Self {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for fi in &r.file_indices {
            let out = compute_dot_names(&fi.functions);
            map.insert(fi.path.clone(), out);
        }
        Self { ver: r.ver, stats: r.stats.clone(), files: map }
    }
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct FolderSummary {
    functions: usize,
    names: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FoldersOutput {
    ver: u8,
    stats: Stats,
    folders: BTreeMap<String, FolderSummary>,
}

impl From<&ScanResult> for FoldersOutput {
    fn from(r: &ScanResult) -> Self {
        let mut map: BTreeMap<String, FolderSummary> = BTreeMap::new();
        for fi in &r.file_indices {
            let dir = std::path::Path::new(&fi.path).parent().map_or_else(
                || ".".to_string(),
                |p| {
                    let s = p.to_string_lossy();
                    if s.is_empty() { ".".to_string() } else { s.to_string() }
                },
            );
            let entry = map.entry(dir).or_default();
            entry.functions += fi.functions.len();
            let dot_names = compute_dot_names(&fi.functions);
            entry.names.extend(dot_names);
        }
        for entry in map.values_mut() {
            entry.names.sort();
            entry.names.dedup();
        }
        Self { ver: r.ver, stats: r.stats.clone(), folders: map }
    }
}

#[derive(Clone, Serialize)]
struct InventoryName {
    file: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

#[derive(Serialize)]
struct InventoryOutput {
    ver: u8,
    stats: Stats,
    functions: Vec<InventoryName>,
    constants: Vec<InventoryName>,
    types: Vec<InventoryName>,
    components: Vec<InventoryName>,
    hooks: Vec<InventoryName>,
    classes: Vec<InventoryName>,
    enums: Vec<InventoryName>,
    exports: Vec<InventoryName>,
}

impl InventoryOutput {
    fn from_result(r: &ScanResult, include_lines: bool) -> Self {
        let mut functions = Vec::new();
        let mut constants = Vec::new();
        let mut types = Vec::new();
        let mut components = Vec::new();
        let mut hooks = Vec::new();
        let mut classes = Vec::new();
        let mut enums = Vec::new();
        let mut exports = Vec::new();

        for fi in &r.file_indices {
            for func in &fi.functions {
                let Some(name) = function_display_name(func) else {
                    continue;
                };
                let item = inventory_item(&fi.path, name, None, func.line, include_lines);
                match func.role(&fi.path) {
                    crate::scan::types::FunctionRole::ReactComponent => {
                        components.push(item.clone());
                    }
                    crate::scan::types::FunctionRole::ReactHook => hooks.push(item.clone()),
                    _ => {}
                }
                functions.push(item);
            }
            for binding in &fi.bindings {
                match binding.kind {
                    BindingKind::Const => {
                        let kind = if is_arrow_const(binding, &fi.functions) {
                            "arrow"
                        } else {
                            "primitive"
                        };
                        constants.push(inventory_item(
                            &fi.path,
                            binding.name.clone(),
                            Some(kind.to_string()),
                            binding.line,
                            include_lines,
                        ));
                    }
                    BindingKind::Enum => {
                        enums.push(inventory_item(
                            &fi.path,
                            binding.name.clone(),
                            None,
                            binding.line,
                            include_lines,
                        ));
                    }
                    _ => {}
                }
            }
            for type_info in &fi.types {
                types.push(inventory_item(
                    &fi.path,
                    type_info.name.clone(),
                    Some(type_info.kind.as_str().to_string()),
                    type_info.line,
                    include_lines,
                ));
            }
            for class in &fi.classes {
                classes.push(inventory_item(
                    &fi.path,
                    class.name.clone(),
                    None,
                    class.line,
                    include_lines,
                ));
            }
            for exp in &fi.exports {
                exports.push(InventoryName {
                    file: fi.path.clone(),
                    name: exp.name.clone(),
                    kind: None,
                    line: None,
                });
            }
        }

        sort_inventory(&mut functions);
        sort_inventory(&mut constants);
        sort_inventory(&mut types);
        sort_inventory(&mut components);
        sort_inventory(&mut hooks);
        sort_inventory(&mut classes);
        sort_inventory(&mut enums);
        sort_inventory(&mut exports);

        Self {
            ver: r.ver,
            stats: r.stats.clone(),
            functions,
            constants,
            types,
            components,
            hooks,
            classes,
            enums,
            exports,
        }
    }
}

fn inventory_item(
    file: &str,
    name: String,
    kind: Option<String>,
    line: u32,
    include_lines: bool,
) -> InventoryName {
    InventoryName { file: file.to_string(), name, kind, line: include_lines.then_some(line) }
}

fn function_display_name(function: &FunctionInfo) -> Option<String> {
    function.name.as_ref().map(|name| {
        function.parent.as_ref().map_or_else(|| name.clone(), |parent| format!("{parent}.{name}"))
    })
}

fn is_arrow_const(binding: &crate::scan::types::BindingInfo, functions: &[FunctionInfo]) -> bool {
    functions.iter().any(|function| {
        matches!(function.kind, FunctionKind::Arrow | FunctionKind::Expression)
            && (function.name.as_deref() == Some(binding.name.as_str())
                || function.line == binding.line)
    })
}

fn sort_inventory(items: &mut [InventoryName]) {
    items.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.name.cmp(&b.name)));
}

fn compute_dot_names(funcs: &[FunctionInfo]) -> Vec<String> {
    let mut out: Vec<String> = funcs
        .iter()
        .filter_map(|function| {
            function.name.as_ref().map(|name| {
                function
                    .parent
                    .as_ref()
                    .map_or_else(|| name.clone(), |parent| format!("{parent}.{name}"))
            })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
#[path = "output_test.rs"]
mod tests;
