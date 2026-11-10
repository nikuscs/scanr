use std::collections::{BTreeSet, HashMap, HashSet};

use oxc::ast::ast::{
    ArrowFunctionBody, ArrowFunctionExpression, BindingPattern, CallExpression, Class,
    ExportDeclaration, ExportDefaultDeclaration, Expression, FormalParameters, Function,
    FunctionBody, FunctionType, JSXElement, JSXFragment, MethodDefinition, MethodDefinitionKind,
    ObjectProperty, PropertyKind, Statement, TSInterfaceDeclaration, TSTypeAliasDeclaration,
};
use oxc::ast_visit::{self, Visit};
use oxc::semantic::Semantic;
use oxc::syntax::scope::{ScopeFlags, ScopeId};
use oxc::syntax::symbol::SymbolFlags;

use super::outline::build_outline;
use crate::scan::outline::{FileOutline, Symbol, SymbolKind};
use crate::scan::types::{
    BindingInfo, BindingKind, ClassInfo, ExportInfo, FunctionContent, FunctionInfo, FunctionKind,
    FunctionKindsFilter, LineIndex, TypeDeclKind, TypeInfo,
};

pub struct ExtractionResult {
    pub functions: Vec<FunctionInfo>,
    pub classes: Vec<ClassInfo>,
    pub bindings: Vec<BindingInfo>,
    pub types: Vec<TypeInfo>,
    pub exports: Vec<ExportInfo>,
    /// Local names exported under any name, plus `default`.
    pub exported_locals: HashSet<String>,
    pub outline: FileOutline,
    pub line_count: u32,
}

pub fn extract_file(
    program: &oxc::ast::ast::Program<'_>,
    semantic: &Semantic<'_>,
    source: &str,
    filter: FunctionKindsFilter,
) -> ExtractionResult {
    let lines = LineIndex::new(source);
    let outlined = build_outline(program, source, &lines);

    let mut collector = Collector {
        functions: Vec::new(),
        classes: Vec::new(),
        types: Vec::new(),
        exported_names: outlined.exported_locals,
        lines: &lines,
        filter,
        in_export: false,
        in_default_export: false,
        in_method: false,
    };
    collector.visit_program(program);
    name_declared_functions(
        &mut collector.functions,
        &outlined.outline,
        &collector.exported_names,
        filter,
    );

    let bindings = extract_bindings(semantic, &lines, &collector.exported_names);
    let functions = enrich_functions(collector.functions, semantic);

    ExtractionResult {
        functions,
        classes: collector.classes,
        bindings,
        types: collector.types,
        exports: outlined.exports,
        exported_locals: collector.exported_names,
        outline: outlined.outline,
        line_count: source.lines().count() as u32,
    }
}

/// Names the functions that a declaration binds rather than names itself:
/// `const f = () => {}`, recognized wrappers such as `memo(() => {})`, and
/// function-valued members such as `{ run: () => {} }` or `run = () => {}`.
/// The outline decides what counts as declared.
fn name_declared_functions(
    records: &mut Vec<FunctionRecord>,
    outline: &FileOutline,
    exported_names: &HashSet<String>,
    filter: FunctionKindsFilter,
) {
    let mut declared = HashMap::new();
    collect_declared_functions(&outline.symbols, None, &mut declared);

    for record in records.iter_mut().filter(|record| record.info.name.is_none()) {
        let Some((name, member_kind)) = declared.remove(&(record.info.line, record.info.col))
        else {
            continue;
        };
        if let Some(kind) = member_kind {
            // A member is reached through its owner; it is not a module export.
            record.info.kind = kind;
            record.info.exported = false;
        } else {
            record.info.exported |= exported_names.contains(&name);
        }
        record.info.name = Some(name);
    }
    records.retain(|record| filter.includes(record.info.kind));
}

/// Maps a function's position to its declared name and, for a member, the
/// method kind of its owner.
fn collect_declared_functions(
    symbols: &[Symbol],
    owner: Option<SymbolKind>,
    declared: &mut HashMap<(u32, u32), (String, Option<FunctionKind>)>,
) {
    for symbol in symbols {
        if let Some(position) = symbol.function_position {
            let member_kind = match (symbol.kind, owner) {
                (SymbolKind::Method, Some(SymbolKind::Class)) => Some(FunctionKind::ClassMethod),
                (SymbolKind::Method, _) => Some(FunctionKind::ObjectMethod),
                _ => None,
            };
            declared.insert(position, (symbol.name.clone(), member_kind));
        }
        collect_declared_functions(&symbol.children, Some(symbol.kind), declared);
    }
}

struct FunctionRecord {
    info: FunctionInfo,
    span_start: u32,
    span_end: u32,
}

struct Collector<'s> {
    functions: Vec<FunctionRecord>,
    classes: Vec<ClassInfo>,
    types: Vec<TypeInfo>,
    exported_names: HashSet<String>,
    lines: &'s LineIndex,
    filter: FunctionKindsFilter,
    in_export: bool,
    in_default_export: bool,
    in_method: bool,
}

impl Collector<'_> {
    #[allow(clippy::too_many_arguments)]
    fn push_function(
        &mut self,
        name: Option<String>,
        kind: FunctionKind,
        is_async: bool,
        is_generator: bool,
        low_value_reason: Option<&'static str>,
        span_start: u32,
        span_end: u32,
    ) {
        if !self.filter.includes(kind) {
            return;
        }
        let exported = self.in_export
            || self.in_default_export
            || name.as_ref().is_some_and(|n| self.exported_names.contains(n));

        self.functions.push(FunctionRecord {
            info: FunctionInfo {
                name,
                parent: None,
                captures: Vec::new(),
                low_value_reason: low_value_reason.map(str::to_string),
                kind,
                exported,
                is_async,
                is_generator,
                content: FunctionContent::Plain,
                line: self.lines.line(span_start),
                col: self.lines.col(span_start),
                line_end: self.lines.line(span_end),
            },
            span_start,
            span_end,
        });
    }

    fn push_type(&mut self, name: &str, kind: TypeDeclKind, span_start: u32) {
        self.types.push(TypeInfo {
            name: name.to_string(),
            kind,
            exported: self.in_export || self.exported_names.contains(name),
            line: self.lines.line(span_start),
            col: self.lines.col(span_start),
        });
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        self.mark_innermost_function_jsx(it.span.start, it.span.end);
        ast_visit::walk::walk_jsx_element(self, it);
    }

    fn visit_jsx_fragment(&mut self, it: &JSXFragment<'a>) {
        self.mark_innermost_function_jsx(it.span.start, it.span.end);
        ast_visit::walk::walk_jsx_fragment(self, it);
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        if let Some(id) = &it.id {
            let name = id.name.to_string();
            self.classes.push(ClassInfo {
                exported: self.in_export
                    || self.in_default_export
                    || self.exported_names.contains(&name),
                name,
                line: self.lines.line(it.span.start),
                line_end: self.lines.line(it.span.end),
            });
        }
        ast_visit::walk::walk_class(self, it);
    }

    fn visit_export_declaration(&mut self, it: &ExportDeclaration<'a>) {
        self.in_export = true;
        ast_visit::walk::walk_export_declaration(self, it);
        self.in_export = false;
    }

    fn visit_export_default_declaration(&mut self, it: &ExportDefaultDeclaration<'a>) {
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
        self.push_function(
            name,
            kind,
            it.r#async,
            it.generator,
            it.body.as_deref().and_then(classify_function_body),
            it.span.start,
            it.span.end,
        );
        ast_visit::walk::walk_function(self, it, flags);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        // Named afterwards by `name_declared_functions`.
        self.push_function(
            None,
            FunctionKind::Arrow,
            it.r#async,
            false,
            classify_arrow_body(&it.body),
            it.span.start,
            it.span.end,
        );
        ast_visit::walk::walk_arrow_function_expression(self, it);
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
        self.push_function(
            name,
            kind,
            func.r#async,
            func.generator,
            func.body.as_deref().and_then(classify_function_body),
            it.span.start,
            it.span.end,
        );
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
                    func.body.as_deref().and_then(classify_function_body),
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

    fn visit_ts_interface_declaration(&mut self, it: &TSInterfaceDeclaration<'a>) {
        self.push_type(it.id.name.as_str(), TypeDeclKind::Interface, it.span.start);
        ast_visit::walk::walk_ts_interface_declaration(self, it);
    }

    fn visit_ts_type_alias_declaration(&mut self, it: &TSTypeAliasDeclaration<'a>) {
        self.push_type(it.id.name.as_str(), TypeDeclKind::TypeAlias, it.span.start);
        ast_visit::walk::walk_ts_type_alias_declaration(self, it);
    }
}

impl Collector<'_> {
    fn mark_innermost_function_jsx(&mut self, span_start: u32, span_end: u32) {
        if let Some(record) = self
            .functions
            .iter_mut()
            .rev()
            .find(|record| record.span_start <= span_start && record.span_end >= span_end)
        {
            record.info.content = FunctionContent::Jsx;
        }
    }
}

fn classify_arrow_body(body: &ArrowFunctionBody<'_>) -> Option<&'static str> {
    match body {
        ArrowFunctionBody::FunctionBody(body) => classify_function_body(body),
        _ => body.as_expression().and_then(classify_expression),
    }
}

fn classify_function_body(body: &FunctionBody<'_>) -> Option<&'static str> {
    match body.statements.as_slice() {
        [] => Some("empty"),
        [Statement::ReturnStatement(statement)] => {
            statement.argument.as_ref().map_or(Some("empty_return"), classify_expression)
        }
        [Statement::ExpressionStatement(statement)] => match &statement.expression {
            Expression::CallExpression(call) if is_direct_passthrough_call(call) => {
                Some("side_effect_wrapper")
            }
            Expression::AwaitExpression(await_expression)
                if matches!(
                    &await_expression.argument,
                    Expression::CallExpression(call) if is_direct_passthrough_call(call)
                ) =>
            {
                Some("side_effect_wrapper")
            }
            _ => None,
        },
        _ => None,
    }
}

fn classify_expression(expression: &Expression<'_>) -> Option<&'static str> {
    match expression {
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::StringLiteral(_) => Some("constant_return"),
        Expression::Identifier(_) => Some("identity_return"),
        Expression::StaticMemberExpression(member) if is_direct_reference(&member.object) => {
            Some("property_return")
        }
        Expression::PrivateFieldExpression(member) if is_direct_reference(&member.object) => {
            Some("property_return")
        }
        Expression::CallExpression(call) if is_direct_passthrough_call(call) => {
            Some("thin_wrapper")
        }
        Expression::AwaitExpression(await_expression)
            if matches!(
                &await_expression.argument,
                Expression::CallExpression(call) if is_direct_passthrough_call(call)
            ) =>
        {
            Some("thin_wrapper")
        }
        _ => None,
    }
}

fn is_direct_passthrough_call(call: &CallExpression<'_>) -> bool {
    is_direct_reference(&call.callee)
        && call
            .arguments
            .iter()
            .all(|argument| argument.as_expression().is_some_and(is_passthrough_argument))
}

fn is_direct_reference(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Identifier(_) => true,
        Expression::StaticMemberExpression(member) => is_direct_reference(&member.object),
        Expression::PrivateFieldExpression(member) => is_direct_reference(&member.object),
        _ => false,
    }
}

fn is_passthrough_argument(expression: &Expression<'_>) -> bool {
    matches!(
        expression,
        Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::Identifier(_)
    ) || matches!(
        expression,
        Expression::StaticMemberExpression(member) if is_direct_reference(&member.object)
    )
}

fn enrich_functions(
    mut records: Vec<FunctionRecord>,
    semantic: &Semantic<'_>,
) -> Vec<FunctionInfo> {
    let parent_indices: Vec<Option<usize>> = records
        .iter()
        .enumerate()
        .map(|(index, child)| {
            records
                .iter()
                .enumerate()
                .filter(|(candidate_index, candidate)| {
                    *candidate_index != index
                        && candidate.info.name.is_some()
                        && candidate.span_start <= child.span_start
                        && candidate.span_end >= child.span_end
                        && (candidate.span_start < child.span_start
                            || candidate.span_end > child.span_end)
                })
                .min_by_key(|(_, candidate)| candidate.span_end - candidate.span_start)
                .map(|(candidate_index, _)| candidate_index)
        })
        .collect();

    for index in 0..records.len() {
        records[index].info.parent =
            parent_indices[index].map(|parent| qualified_name(&records, &parent_indices, parent));
    }

    let scoping = semantic.scoping();
    let mut captures: Vec<BTreeSet<String>> = vec![BTreeSet::new(); records.len()];

    for symbol_id in scoping.symbol_ids() {
        let Some(declaring_function_scope) =
            nearest_function_scope(scoping, scoping.symbol_scope_id(symbol_id))
        else {
            continue;
        };

        for reference in scoping.get_resolved_references(symbol_id) {
            let span = semantic.reference_span(reference);
            let Some(function_index) = records
                .iter()
                .enumerate()
                .filter(|(_, function)| {
                    function.span_start <= span.start && function.span_end >= span.end
                })
                .min_by_key(|(_, function)| function.span_end - function.span_start)
                .map(|(index, _)| index)
            else {
                continue;
            };
            let Some(reference_function_scope) =
                nearest_function_scope(scoping, reference.scope_id())
            else {
                continue;
            };

            if declaring_function_scope != reference_function_scope
                && scope_is_ancestor(scoping, declaring_function_scope, reference_function_scope)
            {
                captures[function_index].insert(scoping.symbol_name(symbol_id).to_string());
            }
        }
    }

    for (record, captures) in records.iter_mut().zip(captures) {
        record.info.captures = captures.into_iter().collect();
    }

    records.into_iter().map(|record| record.info).collect()
}

fn qualified_name(
    records: &[FunctionRecord],
    parent_indices: &[Option<usize>],
    index: usize,
) -> String {
    let mut names = Vec::new();
    let mut current = Some(index);
    while let Some(function_index) = current {
        if let Some(name) = records[function_index].info.name.as_deref() {
            names.push(name);
        }
        current = parent_indices[function_index];
    }
    names.reverse();
    names.join(".")
}

fn nearest_function_scope(scoping: &oxc::semantic::Scoping, scope_id: ScopeId) -> Option<ScopeId> {
    let mut current = Some(scope_id);
    while let Some(id) = current {
        if scoping.scope_flags(id).is_function() {
            return Some(id);
        }
        current = scoping.scope_parent_id(id);
    }
    None
}

fn scope_is_ancestor(
    scoping: &oxc::semantic::Scoping,
    ancestor: ScopeId,
    descendant: ScopeId,
) -> bool {
    let mut current = scoping.scope_parent_id(descendant);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = scoping.scope_parent_id(id);
    }
    false
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
