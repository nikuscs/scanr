//! `tree --outline`: the project tree with each file's nested declarations.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use rayon::prelude::*;

use super::{TreeNode, rel_path, summarize};
use crate::scan::outline::{FileOutline, OutlineView, Reexport, Symbol, SymbolKind};
use crate::scan::rules::is_test_path;
use crate::scan::types::FunctionKindsFilter;
use crate::scan::typescript::parse::process_file;
use crate::scan::{ScanConfig, collect_files};

/// Outlines of the scannable files under `scan_root`, keyed by project-relative path.
pub(super) fn collect_outlines(
    scan_root: &Path,
    project_root: &Path,
    include_tests: bool,
    view: OutlineView,
) -> Result<BTreeMap<String, FileOutline>> {
    let files = collect_files(scan_root, &ScanConfig::default())?;
    let files: Vec<_> = files
        .into_iter()
        .filter(|path| include_tests || !is_test_path(&rel_path(project_root, path)))
        .collect();
    files
        .par_iter()
        .map(|path| {
            let index = process_file(path, project_root, FunctionKindsFilter::All)?;
            Ok((index.path, index.outline.view(view)))
        })
        .collect()
}

pub(super) fn render_outline_tree(
    node: &TreeNode,
    outlines: &BTreeMap<String, FileOutline>,
    lines: &mut Vec<String>,
    max_depth: usize,
    deep: bool,
) {
    if !node.rel_path.is_empty() {
        lines.push(format!("{}/", node.rel_path));
    }
    render_directory(node, outlines, lines, "", 0, max_depth, deep);
}

fn render_directory(
    node: &TreeNode,
    outlines: &BTreeMap<String, FileOutline>,
    lines: &mut Vec<String>,
    indent: &str,
    depth: usize,
    max_depth: usize,
    deep: bool,
) {
    let child_count = node.dirs.len() + node.files.len();

    for (index, dir) in node.dirs.iter().enumerate() {
        let is_last = index + 1 == child_count;
        let connector = connector(is_last);
        if depth + 1 >= max_depth {
            lines.push(format!("{indent}{connector} {}/ {}", dir.name, summarize(dir)));
            continue;
        }
        lines.push(format!("{indent}{connector} {}/", dir.name));
        let child_indent = format!("{indent}{}", continuation(is_last));
        render_directory(dir, outlines, lines, &child_indent, depth + 1, max_depth, deep);
    }

    for (file_index, file) in node.files.iter().enumerate() {
        let is_last = node.dirs.len() + file_index + 1 == child_count;
        lines.push(format!("{indent}{} {}", connector(is_last), file.name));
        let path = if node.rel_path.is_empty() {
            file.name.clone()
        } else {
            format!("{}/{}", node.rel_path, file.name)
        };
        if let Some(outline) = outlines.get(&path) {
            let child_indent = format!("{indent}{}", continuation(is_last));
            lines.extend(render_rows(&outline_rows(outline, &child_indent, deep)));
        }
    }
}

/// One rendered line of a file outline, kept in columns until the file's widths are known.
struct Row {
    label: String,
    tags: String,
    line: Option<u32>,
}

fn outline_rows(outline: &FileOutline, indent: &str, deep: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    let entry_count =
        usize::from(!outline.imports.is_empty()) + outline.symbols.len() + outline.reexports.len();
    let mut entry = 0;
    let mut next_is_last = || {
        entry += 1;
        entry == entry_count
    };

    if !outline.imports.is_empty() {
        rows.push(Row {
            label: format!("{indent}{} imports", connector(next_is_last())),
            tags: outline.imports.join(", "),
            line: None,
        });
    }
    for symbol in &outline.symbols {
        symbol_rows(symbol, indent, next_is_last(), true, deep, &mut rows);
    }
    for reexport in &outline.reexports {
        rows.push(Row {
            label: format!("{indent}{} {}", connector(next_is_last()), reexport_label(reexport)),
            tags: format!(
                "re-exported{} from {}",
                if reexport.type_only { " as type" } else { "" },
                reexport.from
            ),
            line: None,
        });
    }
    rows
}

fn symbol_rows(
    symbol: &Symbol,
    indent: &str,
    is_last: bool,
    top_level: bool,
    deep: bool,
    rows: &mut Vec<Row>,
) {
    let is_leaf_value = symbol.children.is_empty()
        && matches!(
            symbol.kind,
            SymbolKind::Const
                | SymbolKind::Let
                | SymbolKind::Var
                | SymbolKind::Property
                | SymbolKind::EnumMember
        );
    rows.push(Row {
        label: format!("{indent}{} {}", connector(is_last), symbol_label(symbol, deep)),
        tags: symbol_tags(symbol, deep),
        line: (top_level || !is_leaf_value).then_some(symbol.line),
    });
    let child_indent = format!("{indent}{}", continuation(is_last));
    for (index, child) in symbol.children.iter().enumerate() {
        let is_last_child = index + 1 == symbol.children.len();
        symbol_rows(child, &child_indent, is_last_child, false, deep, rows);
    }
}

fn symbol_label(symbol: &Symbol, deep: bool) -> String {
    let call =
        matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor);
    let mut label = symbol.name.clone();
    match &symbol.signature {
        Some(signature) => label.push_str(signature),
        None if call => label.push_str("()"),
        None => {}
    }
    if deep && let Some(via) = &symbol.via {
        label.push_str(" = ");
        label.push_str(via);
    }
    label
}

fn symbol_tags(symbol: &Symbol, deep: bool) -> String {
    let mut tags = Vec::new();
    let kind = match symbol.kind {
        SymbolKind::Function
        | SymbolKind::Method
        | SymbolKind::Constructor
        | SymbolKind::Const
        | SymbolKind::Property
        | SymbolKind::EnumMember => None,
        kind => Some(kind.as_str()),
    };
    if let Some(kind) = kind {
        tags.push(kind.to_string());
    }
    if deep && let Some(extends) = &symbol.extends {
        tags.push(format!("extends {extends}"));
    }
    if symbol.exported {
        tags.push(if symbol.exported_as.is_empty() {
            "exported".to_string()
        } else {
            format!("exported as {}", symbol.exported_as.join(", "))
        });
    }
    tags.join("  ")
}

fn reexport_label(reexport: &Reexport) -> String {
    match (&reexport.namespace, reexport.all) {
        (Some(namespace), _) => format!("* as {namespace}"),
        (None, true) => "*".to_string(),
        (None, false) => reexport.names.join(", "),
    }
}

/// Pads the label and tag columns to the widest row of the file.
fn render_rows(rows: &[Row]) -> Vec<String> {
    let width = |text: &str| text.chars().count();
    let label_width = rows.iter().map(|row| width(&row.label)).max().unwrap_or(0);
    let tags_width = rows.iter().map(|row| width(&row.tags)).max().unwrap_or(0);
    rows.iter()
        .map(|row| {
            let label_padding = " ".repeat(label_width - width(&row.label));
            let tags_padding = " ".repeat(tags_width - width(&row.tags));
            let line = row.line.map(|line| format!("(line {line})")).unwrap_or_default();
            format!("{}{label_padding}  {}{tags_padding}  {line}", row.label, row.tags)
                .trim_end()
                .to_string()
        })
        .collect()
}

const fn connector(is_last: bool) -> &'static str {
    if is_last { "└──" } else { "├──" }
}

const fn continuation(is_last: bool) -> &'static str {
    if is_last { "    " } else { "│   " }
}

#[cfg(test)]
#[path = "outline_test.rs"]
mod tests;
