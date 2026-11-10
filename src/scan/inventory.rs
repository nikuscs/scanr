//! `scan --mode inventory`: flat name lists derived from each file's outline.

use std::collections::HashMap;

use serde::Serialize;

use super::outline::SymbolKind;
use super::types::{EXPORT_REEXPORT, FileIndex, FunctionRole, ScanResult, Stats};

#[derive(Clone, Serialize)]
struct InventoryName {
    file: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    /// Source module of a re-exported name.
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

#[derive(Default, Serialize)]
struct InventoryLists {
    functions: Vec<InventoryName>,
    constants: Vec<InventoryName>,
    variables: Vec<InventoryName>,
    types: Vec<InventoryName>,
    components: Vec<InventoryName>,
    hooks: Vec<InventoryName>,
    classes: Vec<InventoryName>,
    enums: Vec<InventoryName>,
    namespaces: Vec<InventoryName>,
    exports: Vec<InventoryName>,
}

#[derive(Serialize)]
pub struct InventoryOutput {
    ver: u8,
    stats: Stats,
    #[serde(flatten)]
    lists: InventoryLists,
}

impl InventoryOutput {
    pub fn from_result(result: &ScanResult, include_lines: bool) -> Self {
        let mut lists = InventoryLists::default();
        for index in &result.file_indices {
            lists.add_file(index, include_lines);
        }
        for list in [
            &mut lists.functions,
            &mut lists.constants,
            &mut lists.variables,
            &mut lists.types,
            &mut lists.components,
            &mut lists.hooks,
            &mut lists.classes,
            &mut lists.enums,
            &mut lists.namespaces,
            &mut lists.exports,
        ] {
            list.sort_by(|a, b| {
                (&a.file, &a.name, a.line, &a.from).cmp(&(&b.file, &b.name, b.line, &b.from))
            });
        }
        Self { ver: result.ver, stats: result.stats.clone(), lists }
    }
}

impl InventoryLists {
    fn add_file(&mut self, index: &FileIndex, include_lines: bool) {
        let roles: HashMap<(u32, u32), FunctionRole> = index
            .functions
            .iter()
            .map(|function| ((function.line, function.col), function.role(&index.path)))
            .collect();
        let item = |name: &str, kind: Option<&'static str>, line: u32| InventoryName {
            file: index.path.clone(),
            name: name.to_string(),
            kind,
            from: None,
            line: include_lines.then_some(line),
        };

        index.outline.walk(&mut |symbol, path, owner| match symbol.kind {
            // Interface and type-literal method signatures are members, not functions.
            kind if kind.is_callable() && !owner.is_some_and(|owner| owner.kind.is_type()) => {
                let entry = item(path, None, symbol.line);
                match symbol.function_position.and_then(|position| roles.get(&position)) {
                    Some(FunctionRole::ReactComponent) => self.components.push(entry.clone()),
                    Some(FunctionRole::ReactHook) => self.hooks.push(entry.clone()),
                    _ => {}
                }
                self.functions.push(entry);
                // `const f = () => {}` is both a function and an arrow constant.
                match symbol.variable {
                    Some(SymbolKind::Const) => {
                        self.constants.push(item(path, Some("arrow"), symbol.line));
                    }
                    Some(variable) => {
                        self.variables.push(item(path, Some(variable.as_str()), symbol.line));
                    }
                    None => {}
                }
            }
            SymbolKind::Const => self.constants.push(item(path, Some("primitive"), symbol.line)),
            SymbolKind::Let | SymbolKind::Var => {
                self.variables.push(item(path, Some(symbol.kind.as_str()), symbol.line));
            }
            SymbolKind::Interface | SymbolKind::Type => {
                self.types.push(item(path, Some(symbol.kind.as_str()), symbol.line));
            }
            SymbolKind::Class => self.classes.push(item(path, None, symbol.line)),
            SymbolKind::Enum => self.enums.push(item(path, None, symbol.line)),
            SymbolKind::Namespace => self.namespaces.push(item(path, None, symbol.line)),
            _ => {}
        });

        for export in index.exports.iter().filter(|export| export.kind_code != EXPORT_REEXPORT) {
            self.exports.push(InventoryName {
                file: index.path.clone(),
                name: export.name.clone(),
                kind: None,
                from: None,
                line: None,
            });
        }
        for reexport in &index.outline.reexports {
            for name in reexport.exported_names() {
                self.exports.push(InventoryName {
                    file: index.path.clone(),
                    name,
                    kind: None,
                    from: Some(reexport.from.clone()),
                    line: None,
                });
            }
        }
        self.exports.dedup_by(|a, b| a.file == b.file && a.name == b.name && a.from == b.from);
    }
}

#[cfg(test)]
#[path = "inventory_test.rs"]
mod tests;
