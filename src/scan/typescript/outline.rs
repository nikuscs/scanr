//! Builds the [`FileOutline`] of one parsed module.
//!
//! Anonymous callbacks are transparent: a declaration inside one belongs to the
//! nearest named declaration around it, or to the file.

use std::collections::{HashMap, HashSet};

use oxc::ast::ast::{
    AccessorProperty, ArrowFunctionExpression, BindingPattern, CallExpression, Class, ClassType,
    ExportAllDeclaration, ExportDeclaration, ExportDefaultDeclaration, ExportFromDeclaration,
    ExportNamedDeclaration, Expression, Function, FunctionType, ImportDeclaration,
    ImportDeclarationSpecifier, MethodDefinition, MethodDefinitionKind, ObjectExpression,
    ObjectProperty, ObjectPropertyKind, Program, PropertyDefinition, PropertyKey, PropertyKind,
    ReturnStatement, StaticBlock, TSEnumDeclaration, TSExternalModuleDeclaration,
    TSGlobalDeclaration, TSInterfaceDeclaration, TSNamespaceDeclaration, TSSignature, TSType,
    TSTypeAliasDeclaration, VariableDeclaration, VariableDeclarationKind, VariableDeclarator,
};
use oxc::ast_visit::{self, Visit};
use oxc::span::Span;
use oxc::syntax::scope::ScopeFlags;

use crate::scan::outline::{FileOutline, Reexport, Symbol, SymbolKind};
use crate::scan::types::{EXPORT_DEFAULT, EXPORT_NAMED, EXPORT_REEXPORT, ExportInfo, LineIndex};

const DEFAULT_EXPORT: &str = "default";

/// Callees whose result is a function when their first argument is one. Syntax
/// alone cannot tell `memo(fn)` from `items.map(fn)`, so the list is explicit;
/// every other call result stays a variable and records its callee as `via`.
/// A bare name also matches as the last segment, so `React.memo` is `memo`.
const FUNCTION_WRAPPERS: &[&str] = &[
    "Effect.fn",
    "Effect.fnUntraced",
    "cache",
    "debounce",
    "forwardRef",
    "memo",
    "memoize",
    "observer",
    "once",
    "throttle",
    "useCallback",
];

pub struct OutlineExtraction {
    pub outline: FileOutline,
    /// Public surface of the module, in source order and without duplicates.
    pub exports: Vec<ExportInfo>,
    /// Local names that are exported under any name, plus `default`.
    pub exported_locals: HashSet<String>,
}

pub fn build_outline(program: &Program<'_>, source: &str, lines: &LineIndex) -> OutlineExtraction {
    let mut builder = OutlineBuilder {
        source,
        lines,
        roots: Vec::new(),
        frames: Vec::new(),
        function_depth: 0,
        namespace_depth: 0,
        variable_kind: SymbolKind::Const,
        export_mark: None,
        owned_objects: HashSet::new(),
        handed_out_objects: HashSet::new(),
        owned_classes: HashSet::new(),
        object_owned: Vec::new(),
        class_owned: Vec::new(),
        imports: Vec::new(),
        import_bindings: HashMap::new(),
        reexports: Vec::new(),
        specifier_exports: Vec::new(),
        exports: Vec::new(),
        exported_locals: HashSet::new(),
    };
    builder.visit_program(program);
    builder.finish()
}

struct Frame {
    symbol: Symbol,
    /// Function nesting depth when the frame opened.
    function_depth: usize,
}

/// A pending `export` keyword: the symbol attached at `depth` is the exported one.
#[derive(Clone, Copy)]
struct ExportMark {
    depth: usize,
    default: bool,
}

struct ImportBinding {
    from: String,
    /// Imported name, `default`, or `*` for a namespace import.
    imported: String,
}

struct OutlineBuilder<'s> {
    source: &'s str,
    lines: &'s LineIndex,
    roots: Vec<Symbol>,
    frames: Vec<Frame>,
    function_depth: usize,
    namespace_depth: usize,
    variable_kind: SymbolKind,
    export_mark: Option<ExportMark>,
    /// Span starts of object literals and class expressions whose members are symbols.
    owned_objects: HashSet<u32>,
    /// Span starts of object literals a function hands out: its return value, or
    /// the argument of a service constructor such as `Store.of({ ... })`.
    handed_out_objects: HashSet<u32>,
    owned_classes: HashSet<u32>,
    /// Whether the object literal or class being walked owns member symbols.
    object_owned: Vec<bool>,
    class_owned: Vec<bool>,
    imports: Vec<String>,
    import_bindings: HashMap<String, ImportBinding>,
    reexports: Vec<Reexport>,
    /// `export { local as exported }` pairs, resolved once every symbol is known.
    specifier_exports: Vec<(String, String)>,
    exports: Vec<ExportInfo>,
    exported_locals: HashSet<String>,
}

impl OutlineBuilder<'_> {
    fn symbol(&self, name: impl Into<String>, kind: SymbolKind, span: Span) -> Symbol {
        Symbol::new(name.into(), kind, self.lines.line(span.start), self.lines.line(span.end))
    }

    fn position(&self, offset: u32) -> (u32, u32) {
        (self.lines.line(offset), self.lines.col(offset))
    }

    /// Source text of a signature on one line.
    fn signature(&self, span: Span) -> String {
        let text = &self.source[span.start as usize..span.end as usize];
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn callable(&self, symbol: &mut Symbol, function: FunctionValue) {
        symbol.function_position = Some(self.position(function.span.start));
        symbol.signature = Some(self.signature(function.signature));
    }

    fn open(&mut self, symbol: Symbol) {
        self.frames.push(Frame { symbol, function_depth: self.function_depth });
    }

    fn close(&mut self) {
        if let Some(frame) = self.frames.pop() {
            self.attach(frame.symbol);
        }
    }

    fn attach(&mut self, mut symbol: Symbol) {
        if let Some(mark) = self.export_mark
            && mark.depth == self.frames.len()
        {
            self.mark_exported(&mut symbol, mark.default);
        }
        let siblings = self.frames.last_mut().map_or(&mut self.roots, |f| &mut f.symbol.children);
        // An overload signature merges into the declaration that follows it.
        if let Some(previous) = siblings.last_mut()
            && previous.kind.is_callable()
            && previous.function_position.is_none()
            && previous.kind == symbol.kind
            && previous.name == symbol.name
        {
            previous.end_line = symbol.end_line;
            previous.function_position = symbol.function_position;
            previous.exported |= symbol.exported;
            previous.children = symbol.children;
            return;
        }
        siblings.push(symbol);
    }

    fn mark_exported(&mut self, symbol: &mut Symbol, default: bool) {
        symbol.exported = true;
        self.exported_locals.insert(symbol.name.clone());
        if self.namespace_depth > 0 {
            return;
        }
        if default {
            if symbol.name != DEFAULT_EXPORT {
                symbol.exported_as.push(DEFAULT_EXPORT.to_string());
            }
            self.exports.push(ExportInfo { name: symbol.name.clone(), kind_code: EXPORT_DEFAULT });
        } else {
            self.exports.push(ExportInfo { name: symbol.name.clone(), kind_code: EXPORT_NAMED });
        }
    }

    fn with_export_mark(&mut self, default: bool, walk: impl FnOnce(&mut Self)) {
        let previous = self.export_mark.replace(ExportMark { depth: self.frames.len(), default });
        walk(self);
        self.export_mark = previous;
    }

    /// A variable is a local when a function body separates it from its owner.
    /// The callback that initializes a call-built variable is that variable's
    /// own body, so one level is allowed there.
    fn is_local(&self) -> bool {
        let Some(frame) = self.frames.last() else {
            return self.function_depth > 0;
        };
        let own_body = usize::from(frame.symbol.kind.is_variable());
        self.function_depth - frame.function_depth > own_body
    }

    fn member_name(key: &PropertyKey<'_>) -> Option<String> {
        if let PropertyKey::PrivateIdentifier(private) = key {
            return Some(format!("#{}", private.name));
        }
        key.static_name().map(|name| name.to_string())
    }

    /// Symbol for a class field or an object property, callable when its value is a function.
    fn member(&self, name: String, value: Option<&Expression<'_>>, span: Span) -> Symbol {
        let value = value.map(DeclaredValue::of).unwrap_or_default();
        let mut symbol = self.symbol(name, SymbolKind::Property, span);
        symbol.via = value.via;
        if let Some(function) = value.function {
            symbol.kind = SymbolKind::Method;
            self.callable(&mut symbol, function);
        } else {
            symbol.detail = true;
        }
        symbol
    }

    fn own_object(&mut self, value: Option<&Expression<'_>>) {
        if let Some(Expression::ObjectExpression(object)) =
            value.map(Expression::get_inner_expression)
        {
            self.owned_objects.insert(object.span.start);
        }
    }

    fn hand_out(&mut self, expression: &Expression<'_>) {
        if let Expression::ObjectExpression(object) = expression.get_inner_expression() {
            self.handed_out_objects.insert(object.span.start);
        }
    }

    /// Records `key` on the nearest function named `local`, searching the
    /// enclosing declarations from the inside out.
    fn expose(&mut self, local: &str, key: &str) {
        let scopes = self
            .frames
            .iter_mut()
            .rev()
            .map(|frame| &mut frame.symbol.children)
            .chain(std::iter::once(&mut self.roots));
        for scope in scopes {
            let Some(symbol) = scope.iter_mut().rev().find(|symbol| symbol.name == local) else {
                continue;
            };
            if symbol.kind.is_callable() && !symbol.exposed_as.iter().any(|name| name == key) {
                symbol.exposed_as.push(key.to_string());
            }
            return;
        }
    }

    /// Members of an interface or type literal. A member that can be called,
    /// written as a method or as a property with a function type, is part of
    /// the declaration's structure; data properties are detail.
    fn signature_members(&self, signatures: &[TSSignature<'_>]) -> Vec<Symbol> {
        signatures
            .iter()
            .filter_map(|signature| {
                let (key, kind, span, signature) = match signature {
                    TSSignature::TSPropertySignature(property) if !property.computed => {
                        let function_type =
                            property.type_annotation.as_deref().and_then(|annotation| {
                                match &annotation.type_annotation {
                                    TSType::TSFunctionType(function) => Some(function.span),
                                    _ => None,
                                }
                            });
                        let kind = if function_type.is_some() {
                            SymbolKind::Method
                        } else {
                            SymbolKind::Property
                        };
                        (&property.key, kind, property.span, function_type)
                    }
                    TSSignature::TSMethodSignature(method) if !method.computed => {
                        let signature = signature_span(
                            method.type_parameters.as_deref().map(|parameters| parameters.span),
                            method.params.span,
                            method.return_type.as_deref().map(|annotation| annotation.span),
                        );
                        (&method.key, SymbolKind::Method, method.span, Some(signature))
                    }
                    _ => return None,
                };
                let mut symbol = self.symbol(Self::member_name(key)?, kind, span);
                symbol.signature = signature.map(|span| self.signature(span));
                symbol.detail = kind == SymbolKind::Property;
                Some(symbol)
            })
            .fold(Vec::new(), |mut members: Vec<Symbol>, member| {
                // Method overloads collapse into one member.
                if members.last().is_none_or(|previous| {
                    previous.name != member.name || previous.kind != member.kind
                }) {
                    members.push(member);
                }
                members
            })
    }

    fn namespace(&mut self, name: String, span: Span, walk: impl FnOnce(&mut Self)) {
        let symbol = self.symbol(name, SymbolKind::Namespace, span);
        self.open(symbol);
        // The pending export belongs to the namespace, not to what it contains.
        let export_mark = self.export_mark.take();
        self.namespace_depth += 1;
        walk(self);
        self.namespace_depth -= 1;
        self.export_mark = export_mark;
        self.close();
    }

    fn function_scope(&mut self, walk: impl FnOnce(&mut Self)) {
        self.function_depth += 1;
        walk(self);
        self.function_depth -= 1;
    }

    fn finish(mut self) -> OutlineExtraction {
        for (local, exported) in std::mem::take(&mut self.specifier_exports) {
            let kind_code = if exported == DEFAULT_EXPORT { EXPORT_DEFAULT } else { EXPORT_NAMED };
            let mut declared = false;
            for symbol in self.roots.iter_mut().filter(|symbol| symbol.name == local) {
                declared = true;
                symbol.exported = true;
                if exported != local && !symbol.exported_as.contains(&exported) {
                    symbol.exported_as.push(exported.clone());
                }
            }
            if declared {
                self.exported_locals.insert(local);
                self.exports.push(ExportInfo { name: exported, kind_code });
            } else if let Some(binding) = self.import_bindings.get(&local) {
                // `import { a } from './a'; export { a };` is a re-export.
                self.reexports.push(if binding.imported == "*" {
                    Reexport {
                        from: binding.from.clone(),
                        names: Vec::new(),
                        all: true,
                        namespace: Some(exported.clone()),
                        type_only: false,
                    }
                } else {
                    Reexport {
                        from: binding.from.clone(),
                        names: vec![renamed(&binding.imported, &exported)],
                        all: false,
                        namespace: None,
                        type_only: false,
                    }
                });
                self.exports.push(ExportInfo { name: exported, kind_code: EXPORT_REEXPORT });
            } else {
                self.exported_locals.insert(local);
                self.exports.push(ExportInfo { name: exported, kind_code });
            }
        }

        let mut seen = HashSet::new();
        self.exports.retain(|export| seen.insert((export.name.clone(), export.kind_code)));

        OutlineExtraction {
            outline: FileOutline {
                symbols: self.roots,
                reexports: self.reexports,
                imports: self.imports,
            },
            exports: self.exports,
            exported_locals: self.exported_locals,
        }
    }
}

impl<'a> Visit<'a> for OutlineBuilder<'_> {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        let from = it.source.value.to_string();
        if !self.imports.contains(&from) {
            self.imports.push(from.clone());
        }
        for specifier in it.specifiers.iter().flatten() {
            let imported = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(named) => {
                    named.imported.name().to_string()
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => DEFAULT_EXPORT.to_string(),
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => "*".to_string(),
            };
            self.import_bindings.insert(
                specifier.name().to_string(),
                ImportBinding { from: from.clone(), imported },
            );
        }
    }

    fn visit_export_declaration(&mut self, it: &ExportDeclaration<'a>) {
        self.with_export_mark(false, |builder| {
            ast_visit::walk::walk_export_declaration(builder, it);
        });
    }

    fn visit_export_named_declaration(&mut self, it: &ExportNamedDeclaration<'a>) {
        if self.namespace_depth > 0 {
            return;
        }
        for specifier in &it.specifiers {
            self.specifier_exports
                .push((specifier.local.name().to_string(), specifier.exported.name().to_string()));
        }
    }

    fn visit_export_from_declaration(&mut self, it: &ExportFromDeclaration<'a>) {
        let mut names = Vec::new();
        for specifier in &it.specifiers {
            let exported = specifier.exported.name().to_string();
            names.push(renamed(specifier.local.name().as_str(), &exported));
            self.exports.push(ExportInfo { name: exported, kind_code: EXPORT_REEXPORT });
        }
        self.reexports.push(Reexport {
            from: it.source.value.to_string(),
            names,
            all: false,
            namespace: None,
            type_only: it.export_kind.is_type(),
        });
    }

    fn visit_export_all_declaration(&mut self, it: &ExportAllDeclaration<'a>) {
        let namespace = it.exported.as_ref().map(|name| name.name().to_string());
        self.exports.push(ExportInfo {
            name: namespace.clone().unwrap_or_else(|| "*".to_string()),
            kind_code: EXPORT_REEXPORT,
        });
        self.reexports.push(Reexport {
            from: it.source.value.to_string(),
            names: Vec::new(),
            all: true,
            namespace,
            type_only: it.export_kind.is_type(),
        });
    }

    fn visit_export_default_declaration(&mut self, it: &ExportDefaultDeclaration<'a>) {
        self.exported_locals.insert(DEFAULT_EXPORT.to_string());
        self.exports
            .push(ExportInfo { name: DEFAULT_EXPORT.to_string(), kind_code: EXPORT_DEFAULT });

        let Some(expression) = it.declaration.as_expression() else {
            // A function, class, or interface declaration, possibly anonymous.
            self.with_export_mark(true, |builder| {
                ast_visit::walk::walk_export_default_declaration(builder, it);
            });
            return;
        };

        if let Expression::Identifier(identifier) = expression.get_inner_expression() {
            self.specifier_exports.push((identifier.name.to_string(), DEFAULT_EXPORT.to_string()));
            return;
        }

        let value = DeclaredValue::of(expression);
        let mut symbol = self.symbol(DEFAULT_EXPORT, value.kind(SymbolKind::Const), it.span);
        symbol.via = value.via;
        if let Some(function) = value.function {
            self.callable(&mut symbol, function);
        }
        symbol.exported = true;
        if let Some(class) = value.class {
            self.owned_classes.insert(class.start);
        }
        self.own_object(Some(expression));
        self.open(symbol);
        ast_visit::walk::walk_export_default_declaration(self, it);
        self.close();
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        let kind = match it.kind {
            VariableDeclarationKind::Var => SymbolKind::Var,
            VariableDeclarationKind::Let => SymbolKind::Let,
            VariableDeclarationKind::Const
            | VariableDeclarationKind::Using
            | VariableDeclarationKind::AwaitUsing => SymbolKind::Const,
        };
        let previous = std::mem::replace(&mut self.variable_kind, kind);
        ast_visit::walk::walk_variable_declaration(self, it);
        self.variable_kind = previous;
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        let BindingPattern::BindingIdentifier(identifier) = &it.id else {
            // Destructuring declares one symbol per bound name.
            for identifier in it.id.get_binding_identifiers() {
                let mut symbol =
                    self.symbol(identifier.name.as_str(), self.variable_kind, identifier.span);
                symbol.detail = self.is_local();
                self.attach(symbol);
            }
            let export_mark = self.export_mark.take();
            ast_visit::walk::walk_variable_declarator(self, it);
            self.export_mark = export_mark;
            return;
        };

        let value = it.init.as_ref().map(DeclaredValue::of).unwrap_or_default();
        let kind = value.kind(self.variable_kind);
        let mut symbol = self.symbol(identifier.name.as_str(), kind, it.span);
        symbol.via = value.via;
        if let Some(function) = value.function {
            symbol.variable = Some(self.variable_kind);
            self.callable(&mut symbol, function);
        }
        if let Some(class) = value.class {
            self.owned_classes.insert(class.start);
        }
        symbol.detail = kind.is_variable() && self.is_local();
        self.own_object(it.init.as_ref());
        self.open(symbol);
        ast_visit::walk::walk_variable_declarator(self, it);
        self.close();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        let declared = matches!(
            it.r#type,
            FunctionType::FunctionDeclaration | FunctionType::TSDeclareFunction
        );
        if !declared {
            self.function_scope(|builder| ast_visit::walk::walk_function(builder, it, flags));
            return;
        }
        // Only `export default function () {}` declares a function without a name.
        let name = it.id.as_ref().map_or(DEFAULT_EXPORT, |id| id.name.as_str());
        let mut symbol = self.symbol(name, SymbolKind::Function, it.span);
        symbol.signature = Some(self.signature(function_signature(it)));
        if it.body.is_some() {
            symbol.function_position = Some(self.position(it.span.start));
        }
        self.open(symbol);
        self.function_scope(|builder| ast_visit::walk::walk_function(builder, it, flags));
        self.close();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        // `() => ({ ... })` returns the object.
        if let Some(returned) = it.body.as_expression() {
            self.hand_out(returned);
        }
        self.function_scope(|builder| {
            ast_visit::walk::walk_arrow_function_expression(builder, it);
        });
    }

    fn visit_static_block(&mut self, it: &StaticBlock<'a>) {
        self.function_scope(|builder| ast_visit::walk::walk_static_block(builder, it));
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        let extends = it.heritage.as_ref().and_then(|heritage| callee_name(&heritage.expression));
        let walk_members = |builder: &mut Self, owned: bool| {
            builder.class_owned.push(owned);
            ast_visit::walk::walk_class(builder, it);
            builder.class_owned.pop();
        };

        if self.owned_classes.remove(&it.span.start) {
            // The enclosing declaration already opened this class's symbol.
            if let Some(frame) = self.frames.last_mut() {
                frame.symbol.extends = extends;
            }
            walk_members(self, true);
        } else if it.r#type == ClassType::ClassDeclaration {
            // Only `export default class {}` declares a class without a name.
            let name = it.id.as_ref().map_or(DEFAULT_EXPORT, |id| id.name.as_str());
            let mut symbol = self.symbol(name, SymbolKind::Class, it.span);
            symbol.extends = extends;
            self.open(symbol);
            walk_members(self, true);
            self.close();
        } else {
            walk_members(self, false);
        }
    }

    fn visit_method_definition(&mut self, it: &MethodDefinition<'a>) {
        let name = Self::member_name(&it.key).filter(|_| !it.computed);
        let (Some(name), Some(true)) = (name, self.class_owned.last().copied()) else {
            ast_visit::walk::walk_method_definition(self, it);
            return;
        };
        let kind = match it.kind {
            MethodDefinitionKind::Constructor => SymbolKind::Constructor,
            MethodDefinitionKind::Get => SymbolKind::Getter,
            MethodDefinitionKind::Set => SymbolKind::Setter,
            MethodDefinitionKind::Method => SymbolKind::Method,
        };
        if kind == SymbolKind::Constructor {
            // `constructor(private readonly db: Db)` declares class properties.
            for parameter in &it.value.params.items {
                if (parameter.accessibility.is_some() || parameter.readonly)
                    && let Some(name) = parameter.pattern.get_identifier_name()
                {
                    let mut property =
                        self.symbol(name.as_str(), SymbolKind::Property, parameter.span);
                    property.detail = true;
                    self.attach(property);
                }
            }
        }
        let mut symbol = self.symbol(name, kind, it.span);
        symbol.signature = Some(self.signature(function_signature(&it.value)));
        if it.value.body.is_some() {
            symbol.function_position = Some(self.position(it.span.start));
        }
        self.open(symbol);
        ast_visit::walk::walk_method_definition(self, it);
        self.close();
    }

    fn visit_property_definition(&mut self, it: &PropertyDefinition<'a>) {
        let name = Self::member_name(&it.key).filter(|_| !it.computed);
        let (Some(name), Some(true)) = (name, self.class_owned.last().copied()) else {
            ast_visit::walk::walk_property_definition(self, it);
            return;
        };
        let symbol = self.member(name, it.value.as_ref(), it.span);
        self.own_object(it.value.as_ref());
        self.open(symbol);
        ast_visit::walk::walk_property_definition(self, it);
        self.close();
    }

    fn visit_accessor_property(&mut self, it: &AccessorProperty<'a>) {
        let name = Self::member_name(&it.key).filter(|_| !it.computed);
        let (Some(name), Some(true)) = (name, self.class_owned.last().copied()) else {
            ast_visit::walk::walk_accessor_property(self, it);
            return;
        };
        let symbol = self.member(name, it.value.as_ref(), it.span);
        self.open(symbol);
        ast_visit::walk::walk_accessor_property(self, it);
        self.close();
    }

    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        if let Some(argument) = &it.argument {
            self.hand_out(argument);
        }
        ast_visit::walk::walk_return_statement(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        // `Service.of({ ... })` builds a service from its members.
        if let Expression::StaticMemberExpression(member) = it.callee.get_inner_expression()
            && member.property.name == "of"
            && let [argument] = it.arguments.as_slice()
            && let Some(argument) = argument.as_expression()
        {
            self.hand_out(argument);
        }
        ast_visit::walk::walk_call_expression(self, it);
    }

    fn visit_object_expression(&mut self, it: &ObjectExpression<'a>) {
        if self.handed_out_objects.remove(&it.span.start) {
            for property in &it.properties {
                if let ObjectPropertyKind::ObjectProperty(property) = property
                    && !property.computed
                    && !property.shorthand
                    && let Some(key) = Self::member_name(&property.key)
                    && let Expression::Identifier(local) = property.value.get_inner_expression()
                    && local.name != key.as_str()
                {
                    self.expose(local.name.as_str(), &key);
                }
            }
        }
        let owned = self.owned_objects.remove(&it.span.start);
        self.object_owned.push(owned);
        ast_visit::walk::walk_object_expression(self, it);
        self.object_owned.pop();
    }

    fn visit_object_property(&mut self, it: &ObjectProperty<'a>) {
        let name = Self::member_name(&it.key).filter(|_| !it.computed);
        let (Some(name), Some(true)) = (name, self.object_owned.last().copied()) else {
            ast_visit::walk::walk_object_property(self, it);
            return;
        };
        let mut symbol = self.member(name, Some(&it.value), it.span);
        let accessor = match it.kind {
            PropertyKind::Get => Some(SymbolKind::Getter),
            PropertyKind::Set => Some(SymbolKind::Setter),
            PropertyKind::Init => it.method.then_some(SymbolKind::Method),
        };
        if let Some(kind) = accessor {
            // Method shorthand and accessors span the whole property.
            symbol.kind = kind;
            symbol.function_position = Some(self.position(it.span.start));
        }
        self.own_object(Some(&it.value));
        self.open(symbol);
        ast_visit::walk::walk_object_property(self, it);
        self.close();
    }

    fn visit_ts_interface_declaration(&mut self, it: &TSInterfaceDeclaration<'a>) {
        let mut symbol = self.symbol(it.id.name.as_str(), SymbolKind::Interface, it.span);
        symbol.children = self.signature_members(&it.body.body);
        self.attach(symbol);
    }

    fn visit_ts_type_alias_declaration(&mut self, it: &TSTypeAliasDeclaration<'a>) {
        let mut symbol = self.symbol(it.id.name.as_str(), SymbolKind::Type, it.span);
        if let TSType::TSTypeLiteral(literal) = &it.type_annotation {
            symbol.children = self.signature_members(&literal.members);
        }
        self.attach(symbol);
    }

    fn visit_ts_enum_declaration(&mut self, it: &TSEnumDeclaration<'a>) {
        let mut symbol = self.symbol(it.id.name.as_str(), SymbolKind::Enum, it.span);
        symbol.children = it
            .body
            .members
            .iter()
            .map(|member| {
                let mut symbol = self.symbol(
                    member.id.static_name().as_str(),
                    SymbolKind::EnumMember,
                    member.span,
                );
                symbol.detail = true;
                symbol
            })
            .collect();
        self.attach(symbol);
    }

    fn visit_ts_namespace_declaration(&mut self, it: &TSNamespaceDeclaration<'a>) {
        self.namespace(it.id.name.to_string(), it.span, |builder| {
            ast_visit::walk::walk_ts_namespace_declaration(builder, it);
        });
    }

    fn visit_ts_external_module_declaration(&mut self, it: &TSExternalModuleDeclaration<'a>) {
        self.namespace(it.id.value.to_string(), it.span, |builder| {
            ast_visit::walk::walk_ts_external_module_declaration(builder, it);
        });
    }

    fn visit_ts_global_declaration(&mut self, it: &TSGlobalDeclaration<'a>) {
        self.namespace("global".to_string(), it.span, |builder| {
            ast_visit::walk::walk_ts_global_declaration(builder, it);
        });
    }
}

/// A function expression and the span of its parameters and return type.
#[derive(Clone, Copy)]
struct FunctionValue {
    span: Span,
    signature: Span,
}

impl FunctionValue {
    fn of(expression: &Expression<'_>) -> Option<Self> {
        match expression.get_inner_expression() {
            Expression::ArrowFunctionExpression(arrow) => Some(Self {
                span: arrow.span,
                signature: signature_span(
                    arrow.type_parameters.as_deref().map(|parameters| parameters.span),
                    arrow.params.span,
                    arrow.return_type.as_deref().map(|annotation| annotation.span),
                ),
            }),
            Expression::FunctionExpression(function) => {
                Some(Self { span: function.span, signature: function_signature(function) })
            }
            _ => None,
        }
    }
}

fn function_signature(function: &Function<'_>) -> Span {
    signature_span(
        function.type_parameters.as_deref().map(|parameters| parameters.span),
        function.params.span,
        function.return_type.as_deref().map(|annotation| annotation.span),
    )
}

/// From the type parameters, when present, to the end of the return type.
fn signature_span(type_parameters: Option<Span>, params: Span, return_type: Option<Span>) -> Span {
    Span::new(
        type_parameters.map_or(params.start, |span| span.start),
        return_type.map_or(params.end, |span| span.end),
    )
}

/// What the initializer of a declaration makes of it.
#[derive(Default)]
struct DeclaredValue {
    via: Option<String>,
    /// The function that is, or is wrapped into, the declared value.
    function: Option<FunctionValue>,
    class: Option<Span>,
}

impl DeclaredValue {
    fn of(expression: &Expression<'_>) -> Self {
        if let Some(function) = FunctionValue::of(expression) {
            return Self { function: Some(function), ..Self::default() };
        }
        match expression.get_inner_expression() {
            Expression::ClassExpression(class) => {
                Self { class: Some(class.span), ..Self::default() }
            }
            Expression::CallExpression(call) => Self {
                via: callee_name(&call.callee),
                function: wrapped_function(call),
                class: None,
            },
            Expression::NewExpression(new) => Self {
                via: callee_name(&new.callee).map(|callee| format!("new {callee}")),
                ..Self::default()
            },
            // `await load()` and `yield* Cache.make()` still name their producer.
            Expression::AwaitExpression(awaited) => Self::produced_by(&awaited.argument),
            Expression::YieldExpression(yielded) => {
                yielded.argument.as_ref().map(Self::produced_by).unwrap_or_default()
            }
            _ => Self::default(),
        }
    }

    fn produced_by(expression: &Expression<'_>) -> Self {
        Self { via: Self::of(expression).via, ..Self::default() }
    }

    const fn kind(&self, variable: SymbolKind) -> SymbolKind {
        if self.function.is_some() {
            SymbolKind::Function
        } else if self.class.is_some() {
            SymbolKind::Class
        } else {
            variable
        }
    }
}

/// Dotted name of the first callee in a call chain: `Effect.fn` for
/// `Effect.fn('name')(body)`, `z.string` for `z.string().min(1)`.
fn callee_name(expression: &Expression<'_>) -> Option<String> {
    match expression.get_inner_expression() {
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        Expression::ThisExpression(_) => Some("this".to_string()),
        Expression::CallExpression(call) => callee_name(&call.callee),
        Expression::StaticMemberExpression(member) => {
            if matches!(member.object.get_inner_expression(), Expression::CallExpression(_)) {
                return callee_name(&member.object);
            }
            callee_name(&member.object).map(|object| format!("{object}.{}", member.property.name))
        }
        _ => None,
    }
}

fn is_function_wrapper(callee: &str) -> bool {
    FUNCTION_WRAPPERS.iter().any(|wrapper| {
        callee == *wrapper || (!wrapper.contains('.') && callee.rsplit('.').next() == Some(wrapper))
    })
}

/// The function a recognized wrapper call returns: `memo(fn)`, `memo(forwardRef(fn))`.
fn wrapped_function(call: &CallExpression<'_>) -> Option<FunctionValue> {
    if !callee_name(&call.callee).is_some_and(|callee| is_function_wrapper(&callee)) {
        return None;
    }
    let argument = call.arguments.first()?.as_expression()?;
    match argument.get_inner_expression() {
        Expression::CallExpression(inner) => wrapped_function(inner),
        _ => FunctionValue::of(argument),
    }
}

fn renamed(local: &str, exported: &str) -> String {
    if local == exported { local.to_string() } else { format!("{local} as {exported}") }
}

#[cfg(test)]
#[path = "outline_test.rs"]
mod tests;
