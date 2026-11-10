//! Per-file symbol outline: what a file declares and how those declarations nest.
//!
//! The outline is the single source for declaration names. `scan --mode outline`,
//! `scan --mode inventory`, and `tree --outline` are all views over it.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SymbolKind {
    Function,
    Const,
    Let,
    Var,
    Class,
    Method,
    Getter,
    Setter,
    Constructor,
    Property,
    Interface,
    Type,
    Enum,
    EnumMember,
    Namespace,
}

impl SymbolKind {
    pub const ALL: [Self; 15] = [
        Self::Function,
        Self::Const,
        Self::Let,
        Self::Var,
        Self::Class,
        Self::Method,
        Self::Getter,
        Self::Setter,
        Self::Constructor,
        Self::Property,
        Self::Interface,
        Self::Type,
        Self::Enum,
        Self::EnumMember,
        Self::Namespace,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Const => "const",
            Self::Let => "let",
            Self::Var => "var",
            Self::Class => "class",
            Self::Method => "method",
            Self::Getter => "getter",
            Self::Setter => "setter",
            Self::Constructor => "constructor",
            Self::Property => "property",
            Self::Interface => "interface",
            Self::Type => "type",
            Self::Enum => "enum",
            Self::EnumMember => "enumMember",
            Self::Namespace => "namespace",
        }
    }

    /// Kinds that are invoked rather than read.
    pub const fn is_callable(self) -> bool {
        matches!(
            self,
            Self::Function | Self::Method | Self::Getter | Self::Setter | Self::Constructor
        )
    }

    pub const fn is_variable(self) -> bool {
        matches!(self, Self::Const | Self::Let | Self::Var)
    }

    /// Kinds whose members are signatures without a body.
    pub const fn is_type(self) -> bool {
        matches!(self, Self::Interface | Self::Type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Callee that produced the value, such as `Effect.fn` or `new Map`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Parameters and return type of a callable, such as `(id: string): User`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Superclass expression of a class, such as `Context.Service`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exported: bool,
    /// Public names when they differ from `name`, such as `default`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exported_as: Vec<String>,
    /// Keys a function is handed out under when they differ from `name`:
    /// `webhookList` for `webhooks` in `Store.of({ webhookList: webhooks })`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exposed_as: Vec<String>,
    pub line: u32,
    pub end_line: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Self>,
    /// Detail symbols are locals, data properties, and enum members. They
    /// appear only in the deep view.
    #[serde(skip)]
    pub detail: bool,
    /// Variable keyword that declared a function-valued symbol.
    #[serde(skip)]
    pub variable: Option<SymbolKind>,
    /// Line and column of the function node behind a callable symbol.
    #[serde(skip)]
    pub function_position: Option<(u32, u32)>,
}

impl Symbol {
    pub const fn new(name: String, kind: SymbolKind, line: u32, end_line: u32) -> Self {
        Self {
            name,
            kind,
            via: None,
            signature: None,
            extends: None,
            exported: false,
            exported_as: Vec::new(),
            exposed_as: Vec::new(),
            line,
            end_line,
            children: Vec::new(),
            detail: false,
            variable: None,
            function_position: None,
        }
    }

    /// Copy for a view. Without `deep`, a detail symbol survives only when it
    /// still owns a structural descendant, so nesting is never broken.
    fn view(&self, view: OutlineView) -> Option<Self> {
        let children: Vec<Self> =
            self.children.iter().filter_map(|child| child.view(view)).collect();
        if !view.deep && self.detail && children.is_empty() {
            return None;
        }
        let signature = if view.signatures { self.signature.clone() } else { None };
        Some(Self { signature, children, ..self.clone() })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reexport {
    pub from: String,
    /// Re-exported names, written `local as exported` when renamed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// `export * from`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
    /// Namespace name of `export * as name from`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub type_only: bool,
}

impl Reexport {
    /// Public names this statement adds to the module, `*` for a bare `export *`.
    pub fn exported_names(&self) -> Vec<String> {
        if self.all {
            return vec![self.namespace.clone().unwrap_or_else(|| "*".to_string())];
        }
        self.names
            .iter()
            .map(|name| name.rsplit(" as ").next().unwrap_or(name).to_string())
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FileOutline {
    pub symbols: Vec<Symbol>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reexports: Vec<Reexport>,
    /// Imported module specifiers in source order. Deep view only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
}

/// What an outline shows. The default is declarations and functions only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OutlineView {
    /// Add locals, data properties, enum members, and imports.
    pub deep: bool,
    /// Keep the parameters and return type of callables.
    pub signatures: bool,
    /// Keep only the module's public surface: exported symbols and re-exports.
    pub exported: bool,
}

impl FileOutline {
    pub fn view(&self, view: OutlineView) -> Self {
        Self {
            symbols: self
                .symbols
                .iter()
                .filter(|symbol| !view.exported || symbol.exported)
                .filter_map(|symbol| symbol.view(view))
                .collect(),
            reexports: self.reexports.clone(),
            imports: if view.deep { self.imports.clone() } else { Vec::new() },
        }
    }

    /// Visit every symbol depth-first with its dotted path, such as
    /// `make.record`, and the symbol that owns it.
    pub fn walk(&self, visit: &mut impl FnMut(&Symbol, &str, Option<&Symbol>)) {
        fn descend(
            symbols: &[Symbol],
            prefix: &str,
            owner: Option<&Symbol>,
            visit: &mut impl FnMut(&Symbol, &str, Option<&Symbol>),
        ) {
            for symbol in symbols {
                let path = if prefix.is_empty() {
                    symbol.name.clone()
                } else {
                    format!("{prefix}.{}", symbol.name)
                };
                visit(symbol, &path, owner);
                descend(&symbol.children, &path, Some(symbol), visit);
            }
        }
        descend(&self.symbols, "", None, visit);
    }
}

#[cfg(test)]
#[path = "outline_test.rs"]
mod tests;
