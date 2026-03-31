use std::collections::HashSet;

use oxc::ast::ast::{
    ArrowFunctionExpression, BindingPattern, Declaration, ExportDefaultDeclaration,
    ExportDefaultDeclarationKind, ExportNamedDeclaration, Expression, FormalParameters, Function,
    FunctionType, MethodDefinition, MethodDefinitionKind, ObjectProperty, PropertyKind,
    VariableDeclarator,
};
use oxc::ast_visit::{self, Visit};
use oxc::semantic::Semantic;
use oxc::syntax::scope::ScopeFlags;
use oxc::syntax::symbol::SymbolFlags;

use crate::scan::types::{
    BindingInfo, BindingKind, EXPORT_DEFAULT, EXPORT_NAMED, ExportInfo, FunctionInfo, FunctionKind,
    FunctionKindsFilter, LineIndex,
};

pub struct ExtractionResult {
    pub functions: Vec<FunctionInfo>,
    pub bindings: Vec<BindingInfo>,
    pub exports: Vec<ExportInfo>,
}

pub fn extract_file(
    program: &oxc::ast::ast::Program<'_>,
    semantic: &Semantic<'_>,
    source: &str,
    filter: FunctionKindsFilter,
) -> ExtractionResult {
    let lines = LineIndex::new(source);

    let mut collector = Collector {
        functions: Vec::new(),
        exported_names: HashSet::new(),
        exports: Vec::new(),
        lines: &lines,
        filter,
        in_export: false,
        in_default_export: false,
        in_method: false,
    };
    collector.visit_program(program);

    let bindings = extract_bindings(semantic, &lines, &collector.exported_names);

    ExtractionResult { functions: collector.functions, bindings, exports: collector.exports }
}

struct Collector<'s> {
    functions: Vec<FunctionInfo>,
    exported_names: HashSet<String>,
    exports: Vec<ExportInfo>,
    lines: &'s LineIndex,
    filter: FunctionKindsFilter,
    in_export: bool,
    in_default_export: bool,
    in_method: bool,
}

impl Collector<'_> {
    fn push_function(
        &mut self,
        name: Option<String>,
        kind: FunctionKind,
        is_async: bool,
        is_generator: bool,
        span_start: u32,
        span_end: u32,
    ) {
        if !self.filter.includes(kind) {
            return;
        }
        let exported = self.in_export
            || self.in_default_export
            || name.as_ref().is_some_and(|n| self.exported_names.contains(n));

        self.functions.push(FunctionInfo {
            name,
            kind,
            exported,
            is_async,
            is_generator,
            line: self.lines.line(span_start),
            col: self.lines.col(span_start),
            line_end: self.lines.line(span_end),
        });
    }

    fn record_export(&mut self, name: &str, kind_code: u8) {
        self.exported_names.insert(name.to_string());
        self.exports.push(ExportInfo { name: name.to_string(), kind_code });
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_export_named_declaration(&mut self, it: &ExportNamedDeclaration<'a>) {
        // `export { foo, bar }` — specifiers
        for spec in &it.specifiers {
            let local_name = spec.local.to_string();
            self.record_export(&local_name, EXPORT_NAMED);
        }

        // `export function foo() {}` or `export const x = ...` — declaration
        if let Some(decl) = &it.declaration {
            self.collect_declaration_names(decl, EXPORT_NAMED);
        }

        self.in_export = true;
        ast_visit::walk::walk_export_named_declaration(self, it);
        self.in_export = false;
    }

    fn visit_export_default_declaration(&mut self, it: &ExportDefaultDeclaration<'a>) {
        self.record_export("default", EXPORT_DEFAULT);

        // If it's a named function/class, also record that name
        match &it.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    self.record_export(id.name.as_str(), EXPORT_DEFAULT);
                }
            }
            ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                if let Some(id) = &c.id {
                    self.record_export(id.name.as_str(), EXPORT_DEFAULT);
                }
            }
            _ => {}
        }

        self.in_default_export = true;
        ast_visit::walk::walk_export_default_declaration(self, it);
        self.in_default_export = false;
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        let kind = match it.r#type {
            FunctionType::FunctionDeclaration => FunctionKind::Declaration,
            FunctionType::FunctionExpression => {
                // Skip the inner FunctionExpression if we already captured
                // this as a method/object-method/getter/setter/constructor
                if self.in_method {
                    ast_visit::walk::walk_function(self, it, flags);
                    return;
                }
                FunctionKind::Expression
            }
            _ => {
                ast_visit::walk::walk_function(self, it, flags);
                return;
            }
        };
        let name = it.id.as_ref().map(|id| id.name.to_string());
        self.push_function(name, kind, it.r#async, it.generator, it.span.start, it.span.end);
        ast_visit::walk::walk_function(self, it, flags);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        // Name is captured in visit_variable_declarator
        self.push_function(
            None,
            FunctionKind::Arrow,
            it.r#async,
            false,
            it.span.start,
            it.span.end,
        );
        ast_visit::walk::walk_arrow_function_expression(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        // `const foo = () => {}` — give the arrow function its binding name
        if let Some(Expression::ArrowFunctionExpression(_)) = &it.init
            && let BindingPattern::BindingIdentifier(id) = &it.id
        {
            let name = id.name.to_string();
            let prev_count = self.functions.len();
            ast_visit::walk::walk_variable_declarator(self, it);
            if self.functions.len() > prev_count {
                let first_new = &mut self.functions[prev_count];
                if first_new.kind == FunctionKind::Arrow && first_new.name.is_none() {
                    first_new.name = Some(name);
                }
            }
            return;
        }
        ast_visit::walk::walk_variable_declarator(self, it);
    }

    fn visit_method_definition(&mut self, it: &MethodDefinition<'a>) {
        let func = &it.value;
        let kind = match it.kind {
            MethodDefinitionKind::Constructor => FunctionKind::Constructor,
            MethodDefinitionKind::Get => FunctionKind::Getter,
            MethodDefinitionKind::Set => FunctionKind::Setter,
            MethodDefinitionKind::Method => FunctionKind::ClassMethod,
        };
        let name = it.key.name().map(|n| n.to_string());
        self.push_function(name, kind, func.r#async, func.generator, it.span.start, it.span.end);
        self.in_method = true;
        ast_visit::walk::walk_method_definition(self, it);
        self.in_method = false;
    }

    fn visit_object_property(&mut self, it: &ObjectProperty<'a>) {
        if it.method || matches!(it.kind, PropertyKind::Get | PropertyKind::Set) {
            let kind = match it.kind {
                PropertyKind::Get => FunctionKind::Getter,
                PropertyKind::Set => FunctionKind::Setter,
                PropertyKind::Init => FunctionKind::ObjectMethod,
            };
            let name = it.key.name().map(|n| n.to_string());
            if let Expression::FunctionExpression(func) = &it.value {
                self.push_function(
                    name,
                    kind,
                    func.r#async,
                    func.generator,
                    it.span.start,
                    it.span.end,
                );
            }
            self.in_method = true;
            ast_visit::walk::walk_object_property(self, it);
            self.in_method = false;
        } else {
            ast_visit::walk::walk_object_property(self, it);
        }
    }
}

impl Collector<'_> {
    fn collect_declaration_names(&mut self, decl: &Declaration<'_>, kind_code: u8) {
        match decl {
            Declaration::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    self.record_export(id.name.as_str(), kind_code);
                }
            }
            Declaration::ClassDeclaration(c) => {
                if let Some(id) = &c.id {
                    self.record_export(id.name.as_str(), kind_code);
                }
            }
            Declaration::VariableDeclaration(v) => {
                for declarator in &v.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                        self.record_export(id.name.as_str(), kind_code);
                    }
                }
            }
            Declaration::TSEnumDeclaration(e) => {
                self.record_export(e.id.name.as_str(), kind_code);
            }
            _ => {}
        }
    }
}

fn extract_bindings(
    semantic: &Semantic<'_>,
    lines: &LineIndex,
    exported_names: &HashSet<String>,
) -> Vec<BindingInfo> {
    let scoping = semantic.scoping();

    scoping
        .symbol_ids()
        .filter_map(|sym_id| {
            let flags = scoping.symbol_flags(sym_id);

            // Skip type-only: TypeAlias, Interface, TypeParameter, TypeImport (unless also Value)
            if !flags.intersects(SymbolFlags::Value)
                && !flags.intersects(SymbolFlags::CatchVariable)
            {
                return None;
            }

            let kind = flags_to_binding_kind(flags)?;
            let name = scoping.symbol_name(sym_id).to_string();
            let span = scoping.symbol_span(sym_id);
            let ref_count = scoping.get_resolved_reference_ids(sym_id).len();
            let exported = exported_names.contains(&name);

            Some(BindingInfo {
                name,
                kind,
                exported,
                refs: ref_count,
                line: lines.line(span.start),
                col: lines.col(span.start),
            })
        })
        .collect()
}

fn flags_to_binding_kind(flags: SymbolFlags) -> Option<BindingKind> {
    match () {
        () if flags.contains(SymbolFlags::ConstVariable) => Some(BindingKind::Const),
        () if flags.contains(SymbolFlags::BlockScopedVariable) => Some(BindingKind::Let),
        () if flags.contains(SymbolFlags::FunctionScopedVariable) => Some(BindingKind::Var),
        () if flags.contains(SymbolFlags::Function) => Some(BindingKind::Function),
        () if flags.contains(SymbolFlags::Class) => Some(BindingKind::Class),
        () if flags.contains(SymbolFlags::Import) => Some(BindingKind::Import),
        () if flags.contains(SymbolFlags::CatchVariable) => Some(BindingKind::Catch),
        () if flags.intersects(SymbolFlags::RegularEnum | SymbolFlags::ConstEnum) => {
            Some(BindingKind::Enum)
        }
        () => None,
    }
}

#[allow(dead_code)]
pub fn extract_param_names(params: &FormalParameters<'_>) -> Vec<String> {
    params
        .items
        .iter()
        .map(|p| match &p.pattern {
            BindingPattern::BindingIdentifier(id) => id.name.to_string(),
            BindingPattern::ObjectPattern(_) => "{...}".to_string(),
            BindingPattern::ArrayPattern(_) => "[...]".to_string(),
            BindingPattern::AssignmentPattern(a) => a
                .left
                .get_binding_identifier()
                .map_or_else(|| "...".to_string(), |id| id.name.to_string()),
        })
        .collect()
}

#[cfg(test)]
#[path = "extract_test.rs"]
mod tests;
