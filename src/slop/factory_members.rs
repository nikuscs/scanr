//! Members of objects returned by exported factories, and every statically named property access.
use std::collections::{BTreeMap, BTreeSet};

use oxc::ast::ast::{
    ArrowFunctionBody, ArrowFunctionExpression, AssignmentTargetPropertyIdentifier,
    AssignmentTargetPropertyProperty, BindingPattern, BindingProperty, CallExpression,
    ComputedMemberExpression, Declaration, ExportDefaultDeclarationKind, Expression, Function,
    FunctionBody, JSXMemberExpression, ObjectExpression, ObjectPropertyKind, Program, Statement,
    StaticMemberExpression, TSLiteral, TSQualifiedName, TSType,
};
use oxc::ast_visit::{Visit, walk};
use oxc::span::{GetSpan, Span};

use super::types::FactoryMemberFact;

/// Collects the returned members of every exported, function-shaped factory. Name filtering
/// happens in the detector, so `--member-factory` does not change the shared facts.
pub fn collect_members(
    program: &Program<'_>,
    exported_names: &BTreeSet<String>,
    span: impl Fn(Span) -> super::types::SourceSpan,
) -> Vec<FactoryMemberFact> {
    let mut members = Vec::new();
    for statement in &program.body {
        let declaration = match statement {
            Statement::ExportDeclaration(export) => Some(&export.declaration),
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                    &export.declaration
                {
                    collect_function(function, None, &span, &mut members);
                }
                None
            }
            _ => statement.as_declaration(),
        };
        match declaration {
            Some(Declaration::FunctionDeclaration(function)) => {
                collect_function(function, Some(exported_names), &span, &mut members);
            }
            Some(Declaration::VariableDeclaration(variable)) => {
                for declarator in &variable.declarations {
                    let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                        continue;
                    };
                    if !exported_names.contains(id.name.as_str()) {
                        continue;
                    }
                    let object =
                        match declarator.init.as_ref().map(Expression::get_inner_expression) {
                            Some(Expression::ArrowFunctionExpression(arrow)) => arrow_object(arrow),
                            Some(Expression::FunctionExpression(function)) => {
                                function.body.as_ref().and_then(|body| returned_object(body))
                            }
                            _ => None,
                        };
                    if let Some(object) = object {
                        collect_object(object, id.name.as_str(), "", &span, &mut members);
                    }
                }
            }
            _ => {}
        }
    }
    members
}

fn collect_function(
    function: &Function<'_>,
    exported_names: Option<&BTreeSet<String>>,
    span: &impl Fn(Span) -> super::types::SourceSpan,
    members: &mut Vec<FactoryMemberFact>,
) {
    let Some(id) = &function.id else {
        return;
    };
    if exported_names.is_some_and(|names| !names.contains(id.name.as_str())) {
        return;
    }
    if let Some(object) = function.body.as_ref().and_then(|body| returned_object(body)) {
        collect_object(object, id.name.as_str(), "", span, members);
    }
}

fn arrow_object<'a>(arrow: &'a ArrowFunctionExpression<'a>) -> Option<&'a ObjectExpression<'a>> {
    match &arrow.body {
        ArrowFunctionBody::FunctionBody(body) => returned_object(body),
        body => match body.as_expression().map(Expression::get_inner_expression) {
            Some(Expression::ObjectExpression(object)) => Some(object),
            _ => None,
        },
    }
}

/// The object literal of the only top-level `return { ... }` statement.
fn returned_object<'a>(body: &'a FunctionBody<'a>) -> Option<&'a ObjectExpression<'a>> {
    let mut objects = body.statements.iter().filter_map(|statement| match statement {
        Statement::ReturnStatement(statement) => {
            match statement.argument.as_ref().map(Expression::get_inner_expression) {
                Some(Expression::ObjectExpression(object)) => Some(object.as_ref()),
                _ => None,
            }
        }
        _ => None,
    });
    let object = objects.next()?;
    objects.next().is_none().then_some(object)
}

/// Collects callable members and the object literals that contain them; plain data may be
/// serialized or read reflectively, which property reads cannot follow.
fn collect_object(
    object: &ObjectExpression<'_>,
    factory: &str,
    prefix: &str,
    span: &impl Fn(Span) -> super::types::SourceSpan,
    members: &mut Vec<FactoryMemberFact>,
) -> bool {
    let mut callable = false;
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        if property.computed {
            continue;
        }
        let Some(name) = property.key.static_name() else {
            continue;
        };
        let path = format!("{prefix}{name}");
        let position = members.len();
        let keep = match property.value.get_inner_expression() {
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => true,
            Expression::ObjectExpression(child) => {
                collect_object(child, factory, &format!("{path}."), span, members)
            }
            _ => false,
        };
        if keep {
            callable = true;
            members.insert(
                position,
                FactoryMemberFact {
                    factory: factory.to_string(),
                    path,
                    name: name.to_string(),
                    span: span(property.span),
                },
            );
        }
    }
    callable
}

/// Every statically named property read: `a.b`, `a?.b`, `a['b']`, destructuring keys,
/// `<a.B />`, `typeof a.b` and `T['b']`, keyed by name with the start offsets of each access.
/// Also returns the functions whose call results are handed to another call, `f(make())` or
/// `f({ key: make() })`, where a library may consume the whole object.
pub fn collect_accesses(program: &Program<'_>) -> (BTreeMap<String, Vec<u32>>, BTreeSet<String>) {
    let mut collector = AccessCollector::default();
    collector.visit_program(program);
    (collector.accesses, collector.handed_off)
}

#[derive(Default)]
struct AccessCollector {
    accesses: BTreeMap<String, Vec<u32>>,
    handed_off: BTreeSet<String>,
}

impl AccessCollector {
    fn hand_off(&mut self, value: &Expression<'_>) {
        if let Expression::CallExpression(call) = value.get_inner_expression()
            && let Expression::Identifier(callee) = &call.callee
        {
            self.handed_off.insert(callee.name.to_string());
        }
    }

    fn record(&mut self, name: &str, start: u32) {
        self.accesses.entry(name.to_string()).or_default().push(start);
    }
}

impl<'a> Visit<'a> for AccessCollector {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        for argument in &it.arguments {
            let Some(argument) = argument.as_expression() else {
                continue;
            };
            match argument.get_inner_expression() {
                Expression::ObjectExpression(object) => {
                    for property in &object.properties {
                        if let ObjectPropertyKind::ObjectProperty(property) = property {
                            self.hand_off(&property.value);
                        }
                    }
                }
                value => self.hand_off(value),
            }
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        self.record(&it.property.name, it.property.span.start);
        walk::walk_static_member_expression(self, it);
    }

    fn visit_computed_member_expression(&mut self, it: &ComputedMemberExpression<'a>) {
        if let Expression::StringLiteral(literal) = &it.expression {
            self.record(&literal.value, literal.span.start);
        }
        walk::walk_computed_member_expression(self, it);
    }

    fn visit_binding_property(&mut self, it: &BindingProperty<'a>) {
        if let Some(name) = it.key.static_name() {
            self.record(&name, it.key.span().start);
        }
        walk::walk_binding_property(self, it);
    }

    fn visit_assignment_target_property_identifier(
        &mut self,
        it: &AssignmentTargetPropertyIdentifier<'a>,
    ) {
        self.record(&it.binding.name, it.binding.span.start);
        walk::walk_assignment_target_property_identifier(self, it);
    }

    fn visit_assignment_target_property_property(
        &mut self,
        it: &AssignmentTargetPropertyProperty<'a>,
    ) {
        if let Some(name) = it.name.static_name() {
            self.record(&name, it.name.span().start);
        }
        walk::walk_assignment_target_property_property(self, it);
    }

    fn visit_jsx_member_expression(&mut self, it: &JSXMemberExpression<'a>) {
        self.record(&it.property.name, it.property.span.start);
        walk::walk_jsx_member_expression(self, it);
    }

    fn visit_ts_qualified_name(&mut self, it: &TSQualifiedName<'a>) {
        self.record(&it.right.name, it.right.span.start);
        walk::walk_ts_qualified_name(self, it);
    }

    fn visit_ts_indexed_access_type(&mut self, it: &oxc::ast::ast::TSIndexedAccessType<'a>) {
        if let TSType::TSLiteralType(literal) = &it.index_type
            && let TSLiteral::StringLiteral(name) = &literal.literal
        {
            self.record(&name.value, name.span.start);
        }
        walk::walk_ts_indexed_access_type(self, it);
    }
}
