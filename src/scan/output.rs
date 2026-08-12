use std::io::Write;

use serde::Serialize;

use super::types::{FunctionInfo, OutputMode, ScanResult, Stats};

pub fn write_result<W: Write>(
    result: &ScanResult,
    mode: OutputMode,
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
    }
    Ok(())
}

type CompactFunction =
    (String, u32, u32, String, u8, String, Option<String>, Vec<String>, Option<String>);

#[derive(Serialize)]
struct CompactOutput {
    ver: u8,
    stats: Stats,
    /// Functions: [file, line, col, name, exported(0/1), kind, parent, captures, `low_value_reason`]
    f: Vec<CompactFunction>,
    /// Bindings: [file, line, col, name, kind, refs]
    b: Vec<(String, u32, u32, String, String, usize)>,
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
        let total_exports = r.file_indices.iter().map(|fi| fi.exports.len()).sum();
        let total_violations = r.file_indices.iter().map(|fi| fi.violations.len()).sum();
        let mut f = Vec::with_capacity(total_functions);
        let mut b = Vec::with_capacity(total_bindings);
        let mut x = Vec::with_capacity(total_exports);
        let mut viol = Vec::with_capacity(total_violations);

        for fi in &r.file_indices {
            for func in &fi.functions {
                f.push((
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
                b.push((
                    fi.path.clone(),
                    binding.line,
                    binding.col,
                    binding.name.clone(),
                    binding.kind.as_str().to_string(),
                    binding.refs,
                ));
            }
            for exp in &fi.exports {
                x.push((fi.path.clone(), exp.name.clone(), exp.kind_code));
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

        Self { ver: r.ver, stats: r.stats.clone(), f, b, x, viol, err: r.errors.clone() }
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
        let total_exports = r.file_indices.iter().map(|fi| fi.exports.len()).sum();
        let total_violations = r.file_indices.iter().map(|fi| fi.violations.len()).sum();
        let mut functions = Vec::with_capacity(total_functions);
        let mut bindings = Vec::with_capacity(total_bindings);
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
