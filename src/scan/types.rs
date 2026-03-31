use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FunctionKind {
    #[serde(rename = "fn")]
    Declaration,
    #[serde(rename = "arr")]
    Arrow,
    #[serde(rename = "fex")]
    Expression,
    #[serde(rename = "cls")]
    ClassMethod,
    #[serde(rename = "obj")]
    ObjectMethod,
    #[serde(rename = "get")]
    Getter,
    #[serde(rename = "set")]
    Setter,
    #[serde(rename = "ctor")]
    Constructor,
}

impl FunctionKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Declaration => "fn",
            Self::Arrow => "arr",
            Self::Expression => "fex",
            Self::ClassMethod => "cls",
            Self::ObjectMethod => "obj",
            Self::Getter => "get",
            Self::Setter => "set",
            Self::Constructor => "ctor",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Declaration => "FunctionDeclaration",
            Self::Arrow => "ArrowFunctionExpression",
            Self::Expression => "FunctionExpression",
            Self::ClassMethod => "ClassMethod",
            Self::ObjectMethod => "ObjectMethod",
            Self::Getter => "Getter",
            Self::Setter => "Setter",
            Self::Constructor => "Constructor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BindingKind {
    Var,
    Let,
    Const,
    #[allow(dead_code)]
    Param,
    #[serde(rename = "fn")]
    Function,
    Class,
    Import,
    Catch,
    Enum,
}

impl BindingKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Var => "var",
            Self::Let => "let",
            Self::Const => "const",
            Self::Param => "param",
            Self::Function => "fn",
            Self::Class => "class",
            Self::Import => "import",
            Self::Catch => "catch",
            Self::Enum => "enum",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionInfo {
    pub name: Option<String>,
    pub kind: FunctionKind,
    pub exported: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub line: u32,
    pub col: u32,
    pub line_end: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingInfo {
    pub name: String,
    pub kind: BindingKind,
    pub exported: bool,
    pub refs: usize,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportInfo {
    pub name: String,
    pub kind_code: u8,
}

pub const EXPORT_NAMED: u8 = 1;
pub const EXPORT_DEFAULT: u8 = 2;
#[allow(dead_code)]
pub const EXPORT_REEXPORT: u8 = 3;

#[derive(Debug, Clone)]
pub struct FileIndex {
    pub path: String,
    pub functions: Vec<FunctionInfo>,
    pub bindings: Vec<BindingInfo>,
    pub exports: Vec<ExportInfo>,
    pub parse_errors: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub files: usize,
    pub parsed: usize,
    pub skipped: usize,
    pub errors: usize,
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub ver: u8,
    pub root: String,
    pub stats: Stats,
    pub file_indices: Vec<FileIndex>,
    pub errors: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputMode {
    #[default]
    Compact,
    Verbose,
    /// Group by file: { files: { "file": [names...] } }
    Files,
    /// Group by folder: { folders: { "dir": { functions, names } } }
    Folders,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FunctionKindsFilter {
    Top,
    #[clap(name = "top+arrow")]
    TopArrow,
    #[clap(name = "top+arrow+class")]
    TopArrowClass,
    #[default]
    All,
}

impl FunctionKindsFilter {
    pub fn includes(self, kind: FunctionKind) -> bool {
        match self {
            Self::Top => kind == FunctionKind::Declaration,
            Self::TopArrow => {
                matches!(kind, FunctionKind::Declaration | FunctionKind::Arrow)
            }
            Self::TopArrowClass => {
                matches!(
                    kind,
                    FunctionKind::Declaration
                        | FunctionKind::Arrow
                        | FunctionKind::ClassMethod
                        | FunctionKind::Constructor
                        | FunctionKind::Getter
                        | FunctionKind::Setter
                )
            }
            Self::All => true,
        }
    }
}

pub struct LineIndex {
    line_starts: Vec<u32>,
}

#[allow(clippy::cast_possible_truncation)]
impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        Self { line_starts }
    }

    /// Returns 1-based line number for a byte offset.
    pub fn line(&self, offset: u32) -> u32 {
        self.line_starts.partition_point(|&start| start <= offset) as u32
    }

    /// Returns 1-based column number for a byte offset.
    pub fn col(&self, offset: u32) -> u32 {
        let line_idx = self.line_starts.partition_point(|&start| start <= offset);
        let line_start = self.line_starts[line_idx.saturating_sub(1)];
        offset - line_start + 1
    }
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
