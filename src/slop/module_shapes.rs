//! Conservative AST proof that a module adds no logic beyond forwarding exports.
use oxc::ast::ast::{
    Argument, ArrowFunctionBody, BindingIdentifier, BindingPattern, Declaration, Expression,
    FormalParameters, Function, FunctionBody, IdentifierReference, ObjectPropertyKind, Program,
    PropertyKind, Statement, VariableDeclaration,
};
use oxc::semantic::Semantic;

use super::types::ModuleShape;

pub fn classify(program: &Program<'_>, semantic: &Semantic<'_>) -> ModuleShape {
    if pass_through(program, semantic) {
        ModuleShape::PassThrough
    } else if program.body.iter().any(|statement| match statement {
        Statement::ExportNamedDeclaration(export) => !export.specifiers.is_empty(),
        Statement::ExportFromDeclaration(export) => !export.specifiers.is_empty(),
        Statement::ExportDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::ExportDefaultDeclaration(_) => true,
        _ => false,
    }) {
        ModuleShape::Exports
    } else {
        ModuleShape::NoExports
    }
}

fn pass_through(program: &Program<'_>, semantic: &Semantic<'_>) -> bool {
    if !program.directives.is_empty() || program.body.is_empty() {
        return false;
    }
    let locals = program
        .body
        .iter()
        .flat_map(|statement| match statement {
            Statement::VariableDeclaration(variable) => variable
                .declarations
                .iter()
                .filter_map(|item| {
                    if let BindingPattern::BindingIdentifier(identifier) = &item.id {
                        Some(identifier.name.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
            Statement::FunctionDeclaration(function) => {
                function.id.as_ref().map(|id| id.name.as_str()).into_iter().collect()
            }
            _ => Vec::new(),
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut has_export = false;
    for statement in &program.body {
        let valid = match statement {
            Statement::ImportDeclaration(import) => {
                import.specifiers.as_ref().is_some_and(|s| !s.is_empty())
            }
            Statement::ExportAllDeclaration(_) => {
                has_export = true;
                true
            }
            Statement::ExportFromDeclaration(export) => {
                has_export = true;
                !export.specifiers.is_empty()
            }
            Statement::ExportDeclaration(export) => {
                has_export = true;
                forwarding_declaration(&export.declaration, semantic)
            }
            Statement::ExportNamedDeclaration(export) => {
                has_export = true;
                !export.specifiers.is_empty() && export.specifiers.iter().all(|specifier| {
                    matches!(&specifier.local, oxc::ast::ast::ModuleExportName::IdentifierReference(id) if imported(id, semantic) || locals.contains(id.name.as_str()))
                })
            }
            Statement::ExportDefaultDeclaration(export) => {
                has_export = true;
                match &export.declaration {
                    oxc::ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                        forwarding_function(function, semantic)
                    }
                    declaration => declaration.as_expression().is_some_and(|expression| {
                        forwarding_value(expression, semantic) || matches!(expression, Expression::Identifier(id) if locals.contains(id.name.as_str()))
                    }),
                }
            }
            Statement::VariableDeclaration(variable) => forwarding_variable(variable, semantic),
            Statement::FunctionDeclaration(function) => forwarding_function(function, semantic),
            _ => false,
        };
        if !valid {
            return false;
        }
    }
    has_export
}

fn imported(identifier: &IdentifierReference<'_>, semantic: &Semantic<'_>) -> bool {
    identifier
        .reference_id
        .get()
        .and_then(|id| semantic.scoping().get_reference(id).symbol_id())
        .is_some_and(|id| semantic.scoping().symbol_flags(id).is_import())
}

fn forwarding_declaration(declaration: &Declaration<'_>, semantic: &Semantic<'_>) -> bool {
    match declaration {
        Declaration::FunctionDeclaration(function) => forwarding_function(function, semantic),
        Declaration::VariableDeclaration(variable) => forwarding_variable(variable, semantic),
        _ => false,
    }
}

fn forwarding_variable(variable: &VariableDeclaration<'_>, semantic: &Semantic<'_>) -> bool {
    variable.declarations.iter().all(|item| {
        matches!(item.id, BindingPattern::BindingIdentifier(_))
            && item.init.as_ref().is_some_and(|value| forwarding_value(value, semantic))
    })
}

fn imported_callee(expression: &Expression<'_>, semantic: &Semantic<'_>) -> bool {
    match expression {
        Expression::Identifier(identifier) => imported(identifier, semantic),
        Expression::StaticMemberExpression(member) => {
            !member.optional
                && matches!(&member.object, Expression::Identifier(identifier) if imported(identifier, semantic))
        }
        _ => false,
    }
}

fn forwarding_value(expression: &Expression<'_>, semantic: &Semantic<'_>) -> bool {
    match expression.get_inner_expression() {
        Expression::Identifier(identifier) => imported(identifier, semantic),
        Expression::ObjectExpression(object) => {
            !object.properties.is_empty()
                && object.properties.iter().all(|property| {
                    matches!(property, ObjectPropertyKind::ObjectProperty(property)
                if !property.computed && property.kind == PropertyKind::Init
                    && forwarding_value(&property.value, semantic))
                })
        }
        Expression::ArrowFunctionExpression(function) => {
            if function.r#async {
                return false;
            }
            let returned = match &function.body {
                ArrowFunctionBody::FunctionBody(body) => returned_expression(body),
                body => body.as_expression(),
            };
            returned.is_some_and(|value| forwarding_return(value, &function.params, semantic))
        }
        Expression::FunctionExpression(function) => forwarding_function(function, semantic),
        _ => false,
    }
}

fn forwarding_function(function: &Function<'_>, semantic: &Semantic<'_>) -> bool {
    !function.r#async
        && !function.generator
        && function
            .body
            .as_ref()
            .and_then(|body| returned_expression(body))
            .is_some_and(|value| forwarding_return(value, &function.params, semantic))
}

fn returned_expression<'a>(body: &'a FunctionBody<'a>) -> Option<&'a Expression<'a>> {
    if !body.directives.is_empty() {
        return None;
    }
    match body.statements.as_slice() {
        [Statement::ReturnStatement(statement)] => statement.argument.as_ref(),
        _ => None,
    }
}

fn same_binding(
    argument: &IdentifierReference<'_>,
    parameter: &BindingIdentifier<'_>,
    semantic: &Semantic<'_>,
) -> bool {
    argument.reference_id.get().and_then(|id| semantic.scoping().get_reference(id).symbol_id())
        == parameter.symbol_id.get()
}

fn forwarding_return(
    expression: &Expression<'_>,
    params: &FormalParameters<'_>,
    semantic: &Semantic<'_>,
) -> bool {
    if params.items.iter().any(|item| {
        item.initializer.is_some() || !matches!(item.pattern, BindingPattern::BindingIdentifier(_))
    }) {
        return false;
    }
    let expression = expression.get_inner_expression();
    if matches!(expression, Expression::ObjectExpression(_)) {
        return forwarding_value(expression, semantic);
    }
    let Expression::CallExpression(call) = expression else {
        return false;
    };
    let rest_matches = match (&params.rest, call.arguments.last()) {
        (None, _) => true,
        (Some(rest), Some(Argument::SpreadElement(spread))) => {
            matches!((&spread.argument, &rest.rest.argument), (Expression::Identifier(argument), BindingPattern::BindingIdentifier(parameter)) if same_binding(argument, parameter, semantic))
        }
        _ => false,
    };
    !call.optional
        && imported_callee(&call.callee, semantic)
        && rest_matches
        && call.arguments.len() == params.items.len() + usize::from(params.rest.is_some())
        && call.arguments.iter().zip(&params.items).all(|(argument, parameter)| {
            let (Argument::Identifier(argument), BindingPattern::BindingIdentifier(parameter)) =
                (argument, &parameter.pattern)
            else {
                return false;
            };
            same_binding(argument, parameter, semantic)
        })
}
