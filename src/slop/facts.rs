use std::collections::{BTreeMap, BTreeSet};

use oxc::ast::{
    AstKind,
    ast::{
        Argument, ArrowFunctionBody, ArrowFunctionExpression, CallExpression, CatchClause,
        Expression, Function, FunctionBody, IfStatement, ImportDeclaration,
        ImportDeclarationSpecifier, ImportOrExportKind, Program, Statement, StaticMemberExpression,
        TSInterfaceDeclaration, TSSignature, TSType, TSTypeAliasDeclaration, VariableDeclarator,
    },
};
use oxc::ast_visit::Visit;
use oxc::semantic::Semantic;
use oxc::span::{GetSpan, Span};

use crate::scan::types::{FunctionInfo, FunctionRole, LineIndex};
use crate::slop::types::{
    AssertionApi, AssertionBoundary, AssertionFact, AsyncFact, BodyShape, BranchFact,
    CallArgumentFact, CallFact, CallResultUse, CallSiteKey, CastFact, CastKind, CatchEffectKind,
    CatchFact, CommentFact, ConstantCondition, DeclarationFact, DeclarationKind, ExportSurface,
    ExpressionShape, FieldFact, FileFacts, GuardFact, GuardKind, ImportFact, ImportSpecifierKind,
    MemberUseFact, MockFact, MockKind, ModelFact, ModelFieldTypeFact, ModelKind, NonNullProof,
    NonNullProofKind, ProductionExpressionFact, PromiseCatchFact, ReferenceFact, Resolution,
    ReturnShape, ScopeKey, SetupFact, SetupId, SetupKind, SourceSpan, SuiteFact, SuiteId,
    SuppressionDirectiveKind, SymbolKey, SymbolTypeFact, TestBodyShape, TestCaseFact, TestFact,
    TestFramework, TestId, TestMode, TransformChainFact, TransformOp, TypeKey,
};

pub fn collect_facts(
    program: &Program<'_>,
    semantic: &Semantic<'_>,
    source: &str,
    path: &str,
    functions: &[FunctionInfo],
    exported_names: &BTreeSet<String>,
    analysis_complete: bool,
) -> FileFacts {
    let normalized_path = normalize_path(path);
    let lines = LineIndex::new(source);
    let root_suite = SuiteId { path: normalized_path.clone(), registration_start: 0 };
    let mut collector = FactCollector {
        semantic,
        source,
        path: &normalized_path,
        lines: &lines,
        functions,
        exported_names,
        facts: FileFacts {
            path: normalized_path.clone(),
            analysis_complete,
            is_test: is_test_path(&normalized_path),
            is_generated: is_generated_path(&normalized_path, source),
            export_surface: ExportSurface {
                complete: analysis_complete,
                ..ExportSurface::default()
            },
            suites: vec![SuiteFact {
                id: root_suite.clone(),
                parent: None,
                name: None,
                span: SourceSpan {
                    path: normalized_path.clone(),
                    start_byte: 0,
                    end_byte: 0,
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 1,
                },
                callback_span: None,
                resolution_complete: true,
            }],
            ..FileFacts::default()
        },
        scopes: vec![ScopeKey::Module(normalized_path.clone())],
        function_metrics: Vec::new(),
        active_function_metrics: Vec::new(),
        node_stack: Vec::new(),
        loop_stack: Vec::new(),
        callback_contexts: BTreeMap::new(),
        active_callback_contexts: Vec::new(),
        suite_stack: vec![root_suite],
        test_stack: Vec::new(),
        setup_stack: Vec::new(),
    };
    collector.collect_comments(program);
    collector.visit_program(program);
    collector.finish_declarations(semantic);
    collector.collect_semantic_references(semantic);
    collector.sort_all();
    collector.facts
}

#[derive(Default)]
struct FunctionMetric {
    span: Span,
    awaits: Vec<SourceSpan>,
    branches: usize,
    control_depth: usize,
    max_control_depth: usize,
}

#[derive(Debug, Clone)]
enum CallbackContext {
    Suite(SuiteId),
    Test(TestId),
    Setup(SetupId),
}

struct FactCollector<'a, 's> {
    semantic: &'s Semantic<'a>,
    source: &'s str,
    path: &'s str,
    lines: &'s LineIndex,
    functions: &'s [FunctionInfo],
    exported_names: &'s BTreeSet<String>,
    facts: FileFacts,
    scopes: Vec<ScopeKey>,
    function_metrics: Vec<FunctionMetric>,
    active_function_metrics: Vec<usize>,
    node_stack: Vec<AstKind<'a>>,
    loop_stack: Vec<SourceSpan>,
    callback_contexts: BTreeMap<u32, CallbackContext>,
    active_callback_contexts: Vec<Option<CallbackContext>>,
    suite_stack: Vec<SuiteId>,
    test_stack: Vec<TestId>,
    setup_stack: Vec<SetupId>,
}

impl FactCollector<'_, '_> {
    fn span(&self, span: Span) -> SourceSpan {
        SourceSpan {
            path: self.path.to_string(),
            start_byte: span.start,
            end_byte: span.end,
            start_line: self.lines.line(span.start),
            start_column: self.lines.col(span.start),
            end_line: self.lines.line(span.end.saturating_sub(1).max(span.start)),
            end_column: self.lines.col(span.end.saturating_sub(1).max(span.start)),
        }
    }

    fn scope(&self) -> ScopeKey {
        self.scopes.last().cloned().unwrap_or_else(|| ScopeKey::Module(self.path.to_string()))
    }

    fn source_slice(&self, span: Span) -> &str {
        self.source.get(span.start as usize..span.end as usize).unwrap_or_default()
    }

    fn expression_shape(&self, expression: &Expression<'_>) -> ExpressionShape {
        shape_expression(expression, self.semantic, self.path, self.source)
    }

    fn register_callback(&mut self, span: Option<Span>, context: CallbackContext) {
        if let Some(span) = span {
            self.callback_contexts.insert(span.start, context);
        }
    }

    fn enter_callback(&mut self, start: u32) {
        let context = self.callback_contexts.get(&start).cloned();
        match &context {
            Some(CallbackContext::Suite(id)) => self.suite_stack.push(id.clone()),
            Some(CallbackContext::Test(id)) => self.test_stack.push(id.clone()),
            Some(CallbackContext::Setup(id)) => self.setup_stack.push(id.clone()),
            None => {}
        }
        self.active_callback_contexts.push(context);
    }

    fn leave_callback(&mut self) {
        match self.active_callback_contexts.pop().flatten() {
            Some(CallbackContext::Suite(_)) => {
                self.suite_stack.pop();
            }
            Some(CallbackContext::Test(_)) => {
                self.test_stack.pop();
            }
            Some(CallbackContext::Setup(_)) => {
                self.setup_stack.pop();
            }
            None => {}
        }
    }

    fn collect_function_contract(&mut self, function: &Function<'_>) {
        let owner_name = function
            .id
            .as_ref()
            .map(|identifier| identifier.name.to_string())
            .or_else(|| {
                self.functions
                    .iter()
                    .find(|info| info.line == self.lines.line(function.span.start))
                    .and_then(|info| info.name.clone())
            })
            .unwrap_or_else(|| format!("<anonymous@{}>", function.span.start));
        let owner = SymbolKey {
            path: self.path.to_string(),
            declaration_start: function.id.as_ref().map_or(function.span.start, |id| id.span.start),
            name: owner_name,
        };
        self.collect_parameter_types(&function.params, &owner);
        if let Some(body) = &function.body {
            self.collect_production_expression(
                owner,
                function.span,
                body.span,
                &function.params,
                single_return_expression(body),
            );
        }
    }

    fn collect_arrow_contract(&mut self, arrow: &ArrowFunctionExpression<'_>) {
        let owner_name = self
            .functions
            .iter()
            .find(|info| info.line == self.lines.line(arrow.span.start))
            .and_then(|info| info.name.clone())
            .unwrap_or_else(|| format!("<arrow@{}>", arrow.span.start));
        let owner = SymbolKey {
            path: self.path.to_string(),
            declaration_start: arrow.span.start,
            name: owner_name,
        };
        self.collect_parameter_types(&arrow.params, &owner);
        let returned = match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => single_return_expression(body),
            body => body.as_expression(),
        };
        self.collect_production_expression(
            owner,
            arrow.span,
            arrow.body.span(),
            &arrow.params,
            returned,
        );
    }

    fn collect_parameter_types(
        &mut self,
        parameters: &oxc::ast::ast::FormalParameters<'_>,
        owner: &SymbolKey,
    ) {
        let scope = ScopeKey::Function(owner.clone());
        for parameter in &parameters.items {
            let annotation = parameter
                .type_annotation
                .as_ref()
                .map(|annotation| normalize_type_text(self.source_slice(annotation.span)));
            let nullable =
                parameter.optional || annotation.as_deref().is_some_and(type_text_nullable);
            for identifier in parameter.pattern.get_binding_identifiers() {
                let key = SymbolKey {
                    path: self.path.to_string(),
                    declaration_start: identifier.span.start,
                    name: identifier.name.to_string(),
                };
                let proof = if annotation.is_some() && !nullable && parameter.initializer.is_none()
                {
                    Resolution::Resolved(NonNullProof {
                        kind: NonNullProofKind::RequiredParameter,
                        span: self.span(parameter.span),
                        scope: scope.clone(),
                        effective_after_byte: owner.declaration_start,
                    })
                } else {
                    Resolution::Unknown {
                        reason: "parameter is optional, nullable, defaulted, or lacks a complete annotation"
                            .to_string(),
                    }
                };
                self.facts.symbol_types.insert(
                    key.clone(),
                    SymbolTypeFact {
                        key,
                        scope: scope.clone(),
                        primitive: annotation.as_deref().and_then(primitive_type),
                        nullable,
                        annotation_complete: annotation.is_some(),
                        proven_nonnull: proof,
                    },
                );
            }
        }
    }

    fn collect_variable_type(&mut self, declarator: &VariableDeclarator<'_>) {
        let annotation = declarator
            .type_annotation
            .as_ref()
            .map(|annotation| normalize_type_text(self.source_slice(annotation.span)));
        let nullable = annotation.as_deref().is_some_and(type_text_nullable);
        let proof =
            declarator.init.as_ref().and_then(nonnull_initializer_kind).map(|kind| NonNullProof {
                kind,
                span: self.span(declarator.init.as_ref().map_or(declarator.span, GetSpan::span)),
                scope: self.scope(),
                effective_after_byte: declarator
                    .init
                    .as_ref()
                    .map_or(declarator.span.end, |init| init.span().end),
            });
        for identifier in declarator.id.get_binding_identifiers() {
            let key = SymbolKey {
                path: self.path.to_string(),
                declaration_start: identifier.span.start,
                name: identifier.name.to_string(),
            };
            self.facts.symbol_types.insert(
                key.clone(),
                SymbolTypeFact {
                    key,
                    scope: self.scope(),
                    primitive: annotation.as_deref().and_then(primitive_type),
                    nullable,
                    annotation_complete: annotation.is_some(),
                    proven_nonnull: proof.clone().map_or_else(
                        || Resolution::Unknown {
                            reason: "initializer does not syntactically prove a non-null value"
                                .to_string(),
                        },
                        Resolution::Resolved,
                    ),
                },
            );
        }
    }

    fn collect_production_expression(
        &mut self,
        owner: SymbolKey,
        owner_span: Span,
        body_span: Span,
        parameters: &oxc::ast::ast::FormalParameters<'_>,
        returned: Option<&Expression<'_>>,
    ) {
        let parameter_names = parameters
            .items
            .iter()
            .filter_map(|parameter| {
                parameter
                    .pattern
                    .get_binding_identifier()
                    .map(|identifier| identifier.name.to_string())
            })
            .collect::<Vec<_>>();
        let complete_parameters = parameter_names.len() == parameters.items.len();
        let Some(returned_expression) = returned else {
            return;
        };
        let details =
            expression_shape_details(returned_expression, self.semantic, self.path, self.source);
        let eligible =
            complete_parameters && !details.has_mutation && details.shape.complexity >= 3;
        self.facts.production_expressions.push(ProductionExpressionFact {
            owner,
            owner_span: self.span(owner_span),
            expression_span: self.span(returned_expression.span()),
            parameter_names,
            returned: details.shape,
            eligible,
            ambiguity: (!eligible).then(|| {
                if !complete_parameters {
                    "destructured/rest parameters are not normalized".to_string()
                } else if details.has_mutation {
                    "return expression mutates state or a parameter".to_string()
                } else {
                    "return expression is below the structural complexity gate".to_string()
                }
            }),
        });
        let _ = body_span;
    }

    fn collect_comments(&mut self, program: &Program<'_>) {
        for comment in &program.comments {
            let raw = self.source_slice(comment.span);
            let lower = raw.to_ascii_lowercase();
            let directive = if lower.contains("@ts-ignore") {
                Some(SuppressionDirectiveKind::TsIgnore)
            } else if lower.contains("eslint-disable-next-line") {
                Some(SuppressionDirectiveKind::EslintDisableNextLine)
            } else if lower.contains("eslint-disable") {
                Some(SuppressionDirectiveKind::EslintDisable)
            } else {
                None
            };
            let mut lint_rules = directive
                .and_then(|_| raw.split("eslint-disable").nth(1))
                .unwrap_or_default()
                .trim_start_matches("-next-line")
                .trim_matches(|character: char| matches!(character, '*' | '/' | ':' | ' '))
                .split([',', ' '])
                .filter(|rule| !rule.is_empty() && rule.contains('/'))
                .map(str::to_string)
                .collect::<Vec<_>>();
            lint_rules.sort();
            lint_rules.dedup();
            let comment_span = self.span(comment.span);
            let narrates_trivial = comment.attached_to > 0
                && narrates_attached_statement(raw, self.source, comment.attached_to as usize);
            self.facts.comments.push(CommentFact {
                span: comment_span,
                scope: ScopeKey::Module(self.path.to_string()),
                directive,
                lint_rules,
                target: (comment.attached_to > 0)
                    .then(|| self.span(Span::new(comment.attached_to, comment.attached_to))),
                placeholder: ["todo", "fixme", "placeholder"]
                    .iter()
                    .any(|marker| lower.contains(marker)),
                narrates_trivial,
            });
        }
    }

    fn collect_semantic_references(&mut self, semantic: &Semantic<'_>) {
        let scoping = semantic.scoping();
        for symbol_id in scoping.symbol_ids() {
            let declaration = scoping.symbol_span(symbol_id);
            let key = SymbolKey {
                path: self.path.to_string(),
                declaration_start: declaration.start,
                name: scoping.symbol_name(symbol_id).to_string(),
            };
            for reference in scoping.get_resolved_references(symbol_id) {
                let span = semantic.reference_span(reference);
                self.facts.references.push(ReferenceFact {
                    name: key.name.clone(),
                    span: self.span(span),
                    resolved: Resolution::Resolved(key.clone()),
                });
            }
        }
    }

    #[allow(clippy::too_many_lines)] // Finalizes related declaration identities and scopes atomically.
    fn finish_declarations(&mut self, semantic: &Semantic<'_>) {
        for metric in &self.function_metrics {
            let start_line = self.lines.line(metric.span.start);
            let info = self.functions.iter().find(|function| {
                function.line == start_line
                    || (function.line <= start_line && function.line_end >= start_line)
            });
            let name = info.and_then(|function| function.name.clone()).unwrap_or_else(|| {
                format!("<anonymous@{}:{}>", start_line, self.lines.col(metric.span.start))
            });
            let declaration_start = semantic
                .scoping()
                .symbol_ids()
                .filter(|symbol_id| semantic.scoping().symbol_name(*symbol_id) == name)
                .map(|symbol_id| semantic.scoping().symbol_span(symbol_id).start)
                .filter(|start| {
                    let line = self.lines.line(*start);
                    (metric.span.start..=metric.span.end).contains(start) || line == start_line
                })
                .min_by_key(|start| start.abs_diff(metric.span.start))
                .unwrap_or(metric.span.start);
            let key =
                SymbolKey { path: self.path.to_string(), declaration_start, name: name.clone() };
            let role = info.map(|function| function.role(self.path));
            let body_shape = info.and_then(|function| function.low_value_reason.as_deref()).map_or(
                BodyShape::Other,
                |reason| match reason {
                    "empty" | "empty_return" => BodyShape::Empty,
                    "constant_return" => BodyShape::ConstantReturn,
                    "identity_return" => BodyShape::IdentityReturn,
                    "property_return" => BodyShape::PropertyReturn,
                    "thin_wrapper" | "side_effect_wrapper" => BodyShape::PassThroughCall,
                    _ => BodyShape::Other,
                },
            );
            let exported = self.exported_names.contains(&name);
            let full_span = self.span(metric.span);
            if exported {
                self.facts.export_surface.names.insert(name.clone(), full_span.clone());
            }
            self.facts.declarations.push(DeclarationFact {
                key: key.clone(),
                span: full_span,
                body_span: None,
                scope: ScopeKey::Module(self.path.to_string()),
                kind: if role == Some(FunctionRole::ClassMethod) {
                    DeclarationKind::Method
                } else {
                    DeclarationKind::Function
                },
                exported_as: exported.then_some(name).into_iter().collect(),
                ambient: false,
                has_body: true,
                is_async: info.is_some_and(|function| function.is_async),
                is_generator: info.is_some_and(|function| function.is_generator),
                role,
                body_shape,
                parameter_count: None,
                branch_complexity: metric.branches + 1,
                control_nesting: metric.max_control_depth,
                await_spans: metric.awaits.clone(),
            });
        }
        self.facts.declarations.sort_by(|left, right| left.key.cmp(&right.key));
        self.facts.declarations.dedup_by(|left, right| left.key == right.key);
        let function_scopes = self
            .facts
            .declarations
            .iter()
            .filter(|declaration| {
                matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Method)
            })
            .map(|declaration| (declaration.span.clone(), declaration.key.clone()))
            .collect::<Vec<_>>();
        for comment in &mut self.facts.comments {
            if let Some((_, key)) = function_scopes
                .iter()
                .filter(|(span, _)| {
                    span.start_byte <= comment.span.start_byte
                        && span.end_byte >= comment.span.end_byte
                })
                .min_by_key(|(span, _)| span.end_byte - span.start_byte)
            {
                comment.scope = ScopeKey::Function(key.clone());
            }
        }
        for guard in &mut self.facts.guards {
            if let Some((_, key)) = function_scopes
                .iter()
                .filter(|(span, _)| {
                    span.start_byte <= guard.span.start_byte && span.end_byte >= guard.span.end_byte
                })
                .min_by_key(|(span, _)| span.end_byte - span.start_byte)
            {
                guard.scope = ScopeKey::Function(key.clone());
            }
        }
        for symbol_type in self.facts.symbol_types.values_mut() {
            let proof_span = match &symbol_type.proven_nonnull {
                Resolution::Resolved(proof) => &proof.span,
                _ => continue,
            };
            if let Some((_, key)) = function_scopes
                .iter()
                .filter(|(span, _)| {
                    span.start_byte <= proof_span.start_byte && span.end_byte >= proof_span.end_byte
                })
                .min_by_key(|(span, _)| span.end_byte - span.start_byte)
            {
                symbol_type.scope = ScopeKey::Function(key.clone());
                if let Resolution::Resolved(proof) = &mut symbol_type.proven_nonnull {
                    proof.scope = ScopeKey::Function(key.clone());
                }
            }
        }
    }

    fn collect_import(&mut self, import: &ImportDeclaration<'_>) {
        let source = import.source.value.to_string();
        let declaration_type_only = matches!(import.import_kind, ImportOrExportKind::Type);
        match &import.specifiers {
            None => self.facts.imports.push(ImportFact {
                span: self.span(import.span),
                source,
                kind: ImportSpecifierKind::SideEffect,
                imported: None,
                local: None,
                type_only: declaration_type_only,
            }),
            Some(specifiers) => {
                for specifier in specifiers {
                    let (span, kind, imported, local, specifier_type_only) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(specifier) => (
                            specifier.span,
                            ImportSpecifierKind::Named,
                            Some(specifier.imported.name().to_string()),
                            Some(specifier.local.name.to_string()),
                            matches!(specifier.import_kind, ImportOrExportKind::Type),
                        ),
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => (
                            specifier.span,
                            ImportSpecifierKind::Default,
                            Some("default".to_string()),
                            Some(specifier.local.name.to_string()),
                            false,
                        ),
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => (
                            specifier.span,
                            ImportSpecifierKind::Namespace,
                            Some("*".to_string()),
                            Some(specifier.local.name.to_string()),
                            false,
                        ),
                    };
                    self.facts.imports.push(ImportFact {
                        span: self.span(span),
                        source: source.clone(),
                        kind,
                        imported,
                        local,
                        type_only: declaration_type_only || specifier_type_only,
                    });
                }
            }
        }
    }

    fn collect_cast(&mut self, span: Span, operand: Span, kind: CastKind, target_type: String) {
        let source_span = self.span(span);
        self.facts.casts.push(CastFact {
            span: source_span.clone(),
            operand_span: self.span(operand),
            expression_root: source_span,
            scope: self.scope(),
            kind,
            nesting_depth: 1,
            nested_assertion_count: 1,
            target_type,
        });
    }

    fn collect_catch(&mut self, catch: &CatchClause<'_>) {
        let mut effects = Vec::new();
        let mut return_shape = ReturnShape::None;
        for statement in &catch.body.body {
            match statement {
                Statement::ThrowStatement(statement) => {
                    effects.push((CatchEffectKind::Throw, self.span(statement.span)));
                }
                Statement::ReturnStatement(statement) => {
                    return_shape = statement
                        .argument
                        .as_ref()
                        .map_or(ReturnShape::Undefined, classify_return_shape);
                    effects.push((CatchEffectKind::Return, self.span(statement.span)));
                }
                Statement::ExpressionStatement(statement) => match &statement.expression {
                    Expression::CallExpression(call) => {
                        let path = callee_path(&call.callee);
                        let effect = if matches!(path.as_slice(), [root, method] if root == "console" && matches!(method.as_str(), "debug" | "info" | "log" | "warn" | "error"))
                        {
                            CatchEffectKind::Log
                        } else if path.last().is_some_and(|name| {
                            matches!(name.as_str(), "captureException" | "captureError")
                        }) {
                            CatchEffectKind::Telemetry
                        } else {
                            CatchEffectKind::OtherCall
                        };
                        effects.push((effect, self.span(call.span)));
                    }
                    Expression::AssignmentExpression(assignment) => {
                        effects.push((CatchEffectKind::Mutation, self.span(assignment.span)));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        self.facts.catches.push(CatchFact {
            span: self.span(catch.span),
            body_span: self.span(catch.body.span),
            scope: self.scope(),
            parameter_name: catch
                .param
                .as_ref()
                .and_then(|parameter| parameter.pattern.get_binding_identifier())
                .map(|identifier| identifier.name.to_string()),
            top_level_statement_count: catch.body.body.len(),
            effects,
            return_shape,
            can_fall_through: !catch.body.body.last().is_some_and(|statement| {
                matches!(statement, Statement::ThrowStatement(_) | Statement::ReturnStatement(_))
            }),
            has_nested_function: catch.body.body.iter().any(statement_contains_function),
        });
    }

    fn collect_branch(&mut self, statement: &IfStatement<'_>) {
        let condition = boolean_condition(&statement.test);
        let unreachable_span = match condition {
            Some(ConstantCondition::AlwaysTrue) => {
                statement.alternate.as_ref().map(|alternate| self.span(alternate.span()))
            }
            Some(ConstantCondition::AlwaysFalse) => Some(self.span(statement.consequent.span())),
            None => None,
        };
        let condition_span = self.span(statement.test.span());
        let adjacent_placeholder_comment = self
            .facts
            .comments
            .iter()
            .filter(|comment| {
                comment.placeholder && comment.span.end_line + 1 >= condition_span.start_line
            })
            .max_by_key(|comment| comment.span.end_byte)
            .map(|comment| comment.span.clone());
        self.facts.branches.push(BranchFact {
            span: self.span(statement.span),
            condition_span,
            scope: self.scope(),
            condition,
            unreachable_span,
            adjacent_placeholder_comment,
        });
        if let Expression::BinaryExpression(binary) = &statement.test {
            let text = self.source_slice(binary.span).to_ascii_lowercase();
            let kind = if text.contains("typeof") {
                Some(GuardKind::TypeofCheck)
            } else if text.contains("null") || text.contains("undefined") {
                Some(GuardKind::NullCheck)
            } else {
                None
            };
            if let Some(kind) = kind {
                self.facts.guards.push(GuardFact {
                    span: self.span(binary.span),
                    scope: self.scope(),
                    guarded_symbol: direct_expression_identifier(&binary.left)
                        .or_else(|| direct_expression_identifier(&binary.right)),
                    kind,
                });
            }
        }
    }

    fn collect_type_alias(&mut self, alias: &TSTypeAliasDeclaration<'_>) {
        let name = alias.id.name.to_string();
        let exported = self.exported_names.contains(&name);
        let mut fields = Vec::new();
        if let TSType::TSTypeLiteral(literal) = &alias.type_annotation {
            for signature in &literal.members {
                if let TSSignature::TSPropertySignature(property) = signature
                    && let Some(field_name) = property.key.name()
                {
                    let type_text = property
                        .type_annotation
                        .as_ref()
                        .map(|annotation| self.source_slice(annotation.span).to_string())
                        .unwrap_or_default();
                    self.facts.model_field_types.push(ModelFieldTypeFact {
                        model: TypeKey {
                            path: self.path.to_string(),
                            declaration_start: alias.id.span.start,
                            name: name.clone(),
                        },
                        field: field_name.to_string(),
                        span: self.span(property.span),
                        primitive: parsed_primitive_type(&type_text),
                        nullable: type_text_nullable(&normalize_type_text(&type_text)),
                    });
                    fields.push(FieldFact {
                        name: field_name.to_string(),
                        span: self.span(property.span),
                        optional: property.optional,
                        nullable: type_text
                            .split('|')
                            .any(|part| matches!(part.trim(), "null" | "undefined")),
                        primitive: primitive_type(&type_text),
                    });
                }
            }
        }
        fields.sort_by(|left, right| left.span.cmp(&right.span));
        let span = self.span(alias.span);
        if exported {
            self.facts.export_surface.names.insert(name.clone(), span.clone());
        }
        self.facts.models.push(ModelFact {
            key: TypeKey {
                path: self.path.to_string(),
                declaration_start: alias.id.span.start,
                name: name.clone(),
            },
            span,
            kind: ModelKind::TypeAlias,
            exported,
            fields,
            extends: Vec::new(),
        });
        self.push_simple_declaration(
            name,
            alias.span,
            alias.id.span.start,
            DeclarationKind::TypeAlias,
            alias.declare,
            false,
        );
    }

    fn push_simple_declaration(
        &mut self,
        name: String,
        span: Span,
        declaration_start: u32,
        kind: DeclarationKind,
        ambient: bool,
        has_body: bool,
    ) {
        let exported = self.exported_names.contains(&name);
        let source_span = self.span(span);
        if exported {
            self.facts.export_surface.names.insert(name.clone(), source_span.clone());
        }
        self.facts.declarations.push(DeclarationFact {
            key: SymbolKey { path: self.path.to_string(), declaration_start, name: name.clone() },
            span: source_span,
            body_span: None,
            scope: self.scope(),
            kind,
            exported_as: exported.then_some(name).into_iter().collect(),
            ambient,
            has_body,
            is_async: false,
            is_generator: false,
            role: None,
            body_shape: BodyShape::Other,
            parameter_count: None,
            branch_complexity: 0,
            control_nesting: 0,
            await_spans: Vec::new(),
        });
    }

    fn collect_model(&mut self, interface: &TSInterfaceDeclaration<'_>) {
        let name = interface.id.name.to_string();
        let exported = self.exported_names.contains(&name);
        let mut fields = Vec::new();
        for signature in &interface.body.body {
            if let TSSignature::TSPropertySignature(property) = signature
                && let Some(field_name) = property.key.name()
            {
                let type_text = property
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.source_slice(annotation.span).to_string())
                    .unwrap_or_default();
                self.facts.model_field_types.push(ModelFieldTypeFact {
                    model: TypeKey {
                        path: self.path.to_string(),
                        declaration_start: interface.id.span.start,
                        name: name.clone(),
                    },
                    field: field_name.to_string(),
                    span: self.span(property.span),
                    primitive: parsed_primitive_type(&type_text),
                    nullable: type_text_nullable(&normalize_type_text(&type_text)),
                });
                fields.push(FieldFact {
                    name: field_name.to_string(),
                    span: self.span(property.span),
                    optional: property.optional,
                    nullable: type_text
                        .split('|')
                        .any(|part| matches!(part.trim(), "null" | "undefined")),
                    primitive: primitive_type(&type_text),
                });
            }
        }
        fields.sort_by(|left, right| left.span.cmp(&right.span));
        let span = self.span(interface.span);
        if exported {
            self.facts.export_surface.names.insert(name.clone(), span.clone());
        }
        self.facts.models.push(ModelFact {
            key: TypeKey {
                path: self.path.to_string(),
                declaration_start: interface.id.span.start,
                name: name.clone(),
            },
            span: span.clone(),
            kind: ModelKind::Interface,
            exported,
            fields,
            extends: interface
                .extends
                .iter()
                .map(|item| self.source_slice(item.span).to_string())
                .collect(),
        });
        self.facts.declarations.push(DeclarationFact {
            key: SymbolKey {
                path: self.path.to_string(),
                declaration_start: interface.id.span.start,
                name: name.clone(),
            },
            span,
            body_span: Some(self.span(interface.body.span)),
            scope: ScopeKey::Module(self.path.to_string()),
            kind: DeclarationKind::Interface,
            exported_as: exported.then_some(name).into_iter().collect(),
            ambient: interface.declare,
            has_body: true,
            is_async: false,
            is_generator: false,
            role: None,
            body_shape: BodyShape::Other,
            parameter_count: None,
            branch_complexity: 0,
            control_nesting: 0,
            await_spans: Vec::new(),
        });
    }

    fn collect_suite_registration(&mut self, call: &CallExpression<'_>, path: &[String]) {
        if !matches!(path.first().map(String::as_str), Some("describe" | "suite")) {
            return;
        }
        let callback = callback_expression(call);
        let id = SuiteId { path: self.path.to_string(), registration_start: call.span.start };
        let parent = self.suite_stack.last().cloned();
        self.facts.suites.push(SuiteFact {
            id: id.clone(),
            parent,
            name: registration_name(call, self.source),
            span: self.span(call.span),
            callback_span: callback.map(|expression| self.span(expression.span())),
            resolution_complete: callback.is_some(),
        });
        self.register_callback(callback.map(GetSpan::span), CallbackContext::Suite(id));
    }

    fn collect_setup_registration(&mut self, call: &CallExpression<'_>, path: &[String]) {
        let Some(kind) = path.first().and_then(|name| match name.as_str() {
            "beforeEach" => Some(SetupKind::BeforeEach),
            "afterEach" => Some(SetupKind::AfterEach),
            "beforeAll" => Some(SetupKind::BeforeAll),
            "afterAll" => Some(SetupKind::AfterAll),
            _ => None,
        }) else {
            return;
        };
        let callback = callback_expression(call);
        let id = SetupId { path: self.path.to_string(), registration_start: call.span.start };
        let suite = self
            .suite_stack
            .last()
            .cloned()
            .unwrap_or_else(|| SuiteId { path: self.path.to_string(), registration_start: 0 });
        self.facts.setups.push(SetupFact {
            id: id.clone(),
            suite,
            kind,
            registration_span: self.span(call.span),
            callback_span: callback.map(|expression| self.span(expression.span())),
            resolution_complete: callback.is_some(),
        });
        self.register_callback(callback.map(GetSpan::span), CallbackContext::Setup(id));
    }

    fn collect_rich_test(
        &mut self,
        call: &CallExpression<'_>,
        path: &[String],
        mode: TestMode,
        callback: Option<&Expression<'_>>,
    ) {
        let Some(callback) = callback else {
            return;
        };
        let Some(body) = test_body_details(callback, self.semantic, self.path, self.source) else {
            return;
        };
        let id = TestId {
            path: self.path.to_string(),
            callback_start: callback.span().start,
            registration_start: call.span.start,
        };
        let suite = self
            .suite_stack
            .last()
            .cloned()
            .unwrap_or_else(|| SuiteId { path: self.path.to_string(), registration_start: 0 });
        self.facts.test_cases.push(TestCaseFact {
            id: id.clone(),
            suite,
            framework: framework_for_registration(path, &self.facts.imports, self.facts.is_test),
            registration_span: self.span(call.span),
            callback_span: self.span(callback.span()),
            body_span: self.span(body.body_span),
            mode,
            body_shape: body.shape,
            has_snapshot: body.has_snapshot,
            has_unknown_dynamic_call: body.has_unknown_dynamic_call,
        });
        self.register_callback(Some(callback.span()), CallbackContext::Test(id));
    }

    fn collect_assertion_narrowing(&mut self, call: &CallExpression<'_>, path: &[String]) {
        if !matches!(path.first().map(String::as_str), Some("assert" | "invariant")) {
            return;
        }
        let Some(Expression::Identifier(identifier)) =
            call.arguments.first().and_then(Argument::as_expression)
        else {
            return;
        };
        let Some(reference_id) = identifier.reference_id.get() else {
            return;
        };
        let Some(symbol_id) = self.semantic.scoping().get_reference(reference_id).symbol_id()
        else {
            return;
        };
        let key = SymbolKey {
            path: self.path.to_string(),
            declaration_start: self.semantic.scoping().symbol_span(symbol_id).start,
            name: self.semantic.scoping().symbol_name(symbol_id).to_string(),
        };
        let scope = self.scope();
        let proof = NonNullProof {
            kind: NonNullProofKind::AssertionCall,
            span: self.span(call.span),
            scope: scope.clone(),
            effective_after_byte: call.span.end,
        };
        self.facts
            .symbol_types
            .entry(key.clone())
            .and_modify(|symbol| symbol.proven_nonnull = Resolution::Resolved(proof.clone()))
            .or_insert(SymbolTypeFact {
                key,
                scope,
                primitive: None,
                nullable: false,
                annotation_complete: false,
                proven_nonnull: Resolution::Resolved(proof),
            });
    }

    fn collect_assertion(&mut self, call: &CallExpression<'_>) {
        let Some(parsed) = parse_assertion(call) else {
            return;
        };
        let invoked_call = parsed
            .actual
            .and_then(direct_call_key)
            .map(|start_byte| CallSiteKey { path: self.path.to_string(), start_byte });
        let api = self
            .facts
            .imports
            .iter()
            .find(|import| import.local.as_deref() == Some(parsed.root.as_str()))
            .map_or(parsed.api, |import| {
                if import.source == "chai" { AssertionApi::ChaiAssert } else { parsed.api }
            });
        self.facts.assertions.push(AssertionFact {
            span: self.span(call.span),
            test: self.test_stack.last().cloned(),
            api,
            api_resolution: assertion_api_resolution(
                &parsed.root,
                &self.facts.imports,
                self.facts.is_test,
            ),
            matcher: parsed.matcher.clone(),
            negated: parsed.negated,
            async_modifier: parsed.async_modifier.clone(),
            boundary: assertion_boundary(&parsed.matcher, parsed.expected),
            actual: parsed.actual.map(|expression| self.expression_shape(expression)),
            expected: parsed.expected.map(|expression| self.expression_shape(expression)),
            invoked_call,
            invokes: Resolution::Unknown {
                reason: "invoked symbol is resolved in the project index".to_string(),
            },
            is_snapshot: is_snapshot_matcher(&parsed.matcher),
        });
    }

    fn collect_mock(&mut self, call: &CallExpression<'_>, path: &[String]) {
        let Some(kind) = mock_kind(path) else {
            return;
        };
        let root = path.first().map_or("", String::as_str);
        let resolution_complete = matches!(root, "vi" | "jest" | "sinon")
            || self.facts.imports.iter().any(|import| {
                import.local.as_deref() == Some(root)
                    && matches!(
                        import.source.as_str(),
                        "vitest" | "@jest/globals" | "sinon" | "bun:test"
                    )
            });
        if !resolution_complete {
            return;
        }
        let suite = self
            .suite_stack
            .last()
            .cloned()
            .unwrap_or_else(|| SuiteId { path: self.path.to_string(), registration_start: 0 });
        self.facts.mocks.push(MockFact {
            span: self.span(call.span),
            test: self.test_stack.last().cloned(),
            suite,
            setup: self.setup_stack.last().cloned(),
            kind,
            callee: path.join("."),
            resolution_complete,
        });
    }

    fn collect_call(&mut self, call: &CallExpression<'_>) {
        let path = callee_path(&call.callee);
        let span = self.span(call.span);
        let key = CallSiteKey { path: self.path.to_string(), start_byte: call.span.start };
        self.facts.calls.push(CallFact {
            key: key.clone(),
            span: span.clone(),
            scope: self.scope(),
            callee_path: path.clone(),
        });
        self.facts.call_arguments.push(CallArgumentFact {
            call: key.clone(),
            arguments: call
                .arguments
                .iter()
                .filter_map(Argument::as_expression)
                .map(|argument| self.expression_shape(argument))
                .collect(),
            has_spread: call.arguments.iter().any(|argument| argument.as_expression().is_none()),
        });
        let result_use =
            self.node_stack.last().map_or(CallResultUse::Other, |parent| match parent {
                AstKind::AwaitExpression(_) => CallResultUse::Awaited,
                AstKind::ReturnStatement(_) => CallResultUse::Returned,
                AstKind::ExpressionStatement(_) => CallResultUse::FloatingExpressionStatement,
                AstKind::UnaryExpression(unary)
                    if self.source_slice(unary.span).trim_start().starts_with("void") =>
                {
                    CallResultUse::Voided
                }
                AstKind::VariableDeclarator(_) | AstKind::AssignmentExpression(_) => {
                    CallResultUse::Assigned
                }
                AstKind::CallExpression(_) | AstKind::NewExpression(_) => CallResultUse::Argument,
                AstKind::IfStatement(_) | AstKind::ConditionalExpression(_) => {
                    CallResultUse::Condition
                }
                _ => CallResultUse::Other,
            });
        self.facts.async_calls.push(AsyncFact {
            key,
            span: span.clone(),
            scope: self.scope(),
            callee_path: path.clone(),
            callee_symbol: Resolution::Unknown {
                reason: "project call target is resolved in Phase 3".to_string(),
            },
            result_use,
            nearest_loop: self.loop_stack.last().cloned(),
            await_span: None,
        });
        let callback = callback_expression(call);
        let callback_span = callback.map(GetSpan::span);
        if path.last().is_some_and(|name| name == "catch") {
            self.facts.promise_catches.push(PromiseCatchFact {
                call_span: span.clone(),
                callback_span: callback_span.map(|callback| self.span(callback)),
                scope: self.scope(),
                callback: Resolution::Unknown {
                    reason: "callback effects require the exact-detector control-flow summarizer"
                        .to_string(),
                },
            });
        }
        let operations = transform_operations_for_call(call);
        if operations.len() >= 2 {
            self.facts.transform_chains.push(TransformChainFact {
                span: span.clone(),
                scope: self.scope(),
                root_symbol: path.first().cloned(),
                operations,
            });
        }
        self.collect_suite_registration(call, &path);
        self.collect_setup_registration(call, &path);
        self.collect_assertion_narrowing(call, &path);
        self.collect_assertion(call);
        self.collect_mock(call, &path);
        if let Some((mode, name, callback_span)) = test_registration(call, &path, self.source) {
            self.collect_rich_test(call, &path, mode, callback);
            let body = callback.and_then(|expression| {
                test_body_details(expression, self.semantic, self.path, self.source)
            });
            self.facts.tests.push(TestFact {
                call_span: span,
                name,
                mode,
                callback_span: callback_span.map(|callback| self.span(callback)),
                callback_resolution_complete: callback_span.is_some(),
                assertion_spans: Vec::new(),
                mock_spans: Vec::new(),
                body_canonical: body.as_ref().map(|details| details.shape.canonical.clone()),
                literal_vector: body.map_or_else(Vec::new, |details| details.shape.literal_vector),
            });
        }
    }

    fn collect_member(&mut self, member: &StaticMemberExpression<'_>) {
        self.facts.member_uses.push(MemberUseFact {
            span: self.span(member.span),
            scope: self.scope(),
            base_name: expression_root_name(&member.object),
            static_member: Some(member.property.name.to_string()),
        });
    }

    fn sort_all(&mut self) {
        self.facts.comments.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.catches.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.promise_catches.sort_by(|left, right| left.call_span.cmp(&right.call_span));
        self.facts.casts.sort_by(|left, right| left.span.cmp(&right.span));
        let casts = self.facts.casts.clone();
        for cast in &mut self.facts.casts {
            let mut containers = casts
                .iter()
                .filter(|candidate| {
                    candidate.span.start_byte <= cast.span.start_byte
                        && candidate.span.end_byte >= cast.span.end_byte
                })
                .collect::<Vec<_>>();
            containers.sort_by_key(|candidate| candidate.span.end_byte - candidate.span.start_byte);
            cast.nesting_depth = containers.len().try_into().unwrap_or(u16::MAX);
            if let Some(root) = containers.last() {
                cast.expression_root = root.span.clone();
                cast.nested_assertion_count = casts
                    .iter()
                    .filter(|candidate| {
                        root.span.start_byte <= candidate.span.start_byte
                            && root.span.end_byte >= candidate.span.end_byte
                    })
                    .count()
                    .try_into()
                    .unwrap_or(u16::MAX);
            }
        }
        self.facts.branches.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.guards.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.transform_chains.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.async_calls.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.calls.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.call_arguments.sort_by(|left, right| left.call.cmp(&right.call));
        self.facts.suites.sort_by(|left, right| left.id.cmp(&right.id));
        self.facts.setups.sort_by(|left, right| left.id.cmp(&right.id));
        self.facts.test_cases.sort_by(|left, right| left.id.cmp(&right.id));
        self.facts.assertions.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.mocks.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts
            .production_expressions
            .sort_by(|left, right| left.expression_span.cmp(&right.expression_span));
        self.facts.tests.sort_by(|left, right| left.call_span.cmp(&right.call_span));
        self.facts.imports.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.member_uses.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.dynamic_import_spans.sort();
        self.facts.dynamic_import_spans.dedup();
        self.facts.references.sort_by(|left, right| left.span.cmp(&right.span));
        self.facts.models.sort_by(|left, right| left.key.cmp(&right.key));
        self.facts.model_field_types.sort_by(|left, right| {
            (&left.model, &left.field, &left.span).cmp(&(&right.model, &right.field, &right.span))
        });

        for production in &mut self.facts.production_expressions {
            if let Some(declaration) = self
                .facts
                .declarations
                .iter()
                .filter(|declaration| {
                    matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Method)
                        && declaration.span.start_byte <= production.expression_span.start_byte
                        && declaration.span.end_byte >= production.expression_span.end_byte
                })
                .min_by_key(|declaration| declaration.span.end_byte - declaration.span.start_byte)
            {
                production.owner = declaration.key.clone();
            }
        }

        for test in &mut self.facts.tests {
            let Some(callback) = &test.callback_span else { continue };
            test.assertion_spans.extend(
                self.facts
                    .assertions
                    .iter()
                    .filter(|assertion| {
                        assertion.span.start_byte >= callback.start_byte
                            && assertion.span.end_byte <= callback.end_byte
                    })
                    .map(|assertion| assertion.span.clone()),
            );
            test.mock_spans.extend(
                self.facts
                    .mocks
                    .iter()
                    .filter(|mock| {
                        mock.span.start_byte >= callback.start_byte
                            && mock.span.end_byte <= callback.end_byte
                    })
                    .map(|mock| mock.span.clone()),
            );
            test.assertion_spans.sort();
            test.assertion_spans.dedup();
            test.mock_spans.sort();
            test.mock_spans.dedup();
        }
    }
}

impl<'a> Visit<'a> for FactCollector<'a, '_> {
    #[allow(clippy::too_many_lines)] // One visitor dispatch guarantees one AST traversal.
    fn enter_node(&mut self, kind: AstKind<'a>) {
        if is_branch(kind)
            && let Some(index) = self.active_function_metrics.last().copied()
        {
            let metric = &mut self.function_metrics[index];
            metric.branches += 1;
            if is_control(kind) {
                metric.control_depth += 1;
                metric.max_control_depth = metric.max_control_depth.max(metric.control_depth);
            }
        }
        match kind {
            AstKind::Function(function) => {
                let name = function.id.as_ref().map_or_else(
                    || format!("<anonymous@{}>", function.span.start),
                    |identifier| identifier.name.to_string(),
                );
                let key = SymbolKey {
                    path: self.path.to_string(),
                    declaration_start: function.span.start,
                    name,
                };
                self.scopes.push(ScopeKey::Function(key));
                let index = self.function_metrics.len();
                self.function_metrics
                    .push(FunctionMetric { span: function.span, ..FunctionMetric::default() });
                self.active_function_metrics.push(index);
                self.collect_function_contract(function);
                self.enter_callback(function.span.start);
            }
            AstKind::ArrowFunctionExpression(arrow) => {
                let key = SymbolKey {
                    path: self.path.to_string(),
                    declaration_start: arrow.span.start,
                    name: format!("<arrow@{}>", arrow.span.start),
                };
                self.scopes.push(ScopeKey::Function(key));
                let index = self.function_metrics.len();
                self.function_metrics
                    .push(FunctionMetric { span: arrow.span, ..FunctionMetric::default() });
                self.active_function_metrics.push(index);
                self.collect_arrow_contract(arrow);
                self.enter_callback(arrow.span.start);
            }
            AstKind::ImportDeclaration(import) => self.collect_import(import),
            AstKind::ExportAllDeclaration(export) => {
                let source = export.source.value.to_string();
                self.facts.export_surface.unknown_star_reexports.push(source);
                self.facts.export_surface.complete = false;
            }
            AstKind::ExportDefaultDeclaration(export) => {
                self.facts.export_surface.default = Some(self.span(export.span));
            }
            AstKind::ExportNamedDeclaration(export) => {
                for specifier in &export.specifiers {
                    self.facts
                        .export_surface
                        .names
                        .insert(specifier.exported.name().to_string(), self.span(specifier.span));
                }
            }
            AstKind::ImportExpression(import) => {
                self.facts.dynamic_import_spans.push(self.span(import.span));
                self.facts.export_surface.complete = false;
            }
            AstKind::CatchClause(catch) => self.collect_catch(catch),
            AstKind::IfStatement(statement) => self.collect_branch(statement),
            AstKind::TSAsExpression(expression) => {
                let target =
                    self.source_slice(expression.type_annotation.span()).trim().to_string();
                let kind = if target == "any" { CastKind::AsAny } else { CastKind::OtherAs };
                self.collect_cast(expression.span, expression.expression.span(), kind, target);
            }
            AstKind::TSTypeAssertion(expression) => {
                let target =
                    self.source_slice(expression.type_annotation.span()).trim().to_string();
                let kind = if target == "any" {
                    CastKind::TypeAssertionAny
                } else {
                    CastKind::OtherTypeAssertion
                };
                self.collect_cast(expression.span, expression.expression.span(), kind, target);
            }
            AstKind::TSNonNullExpression(expression) => self.collect_cast(
                expression.span,
                expression.expression.span(),
                CastKind::NonNull,
                String::new(),
            ),
            AstKind::TSInterfaceDeclaration(interface) => self.collect_model(interface),
            AstKind::TSTypeAliasDeclaration(alias) => self.collect_type_alias(alias),
            AstKind::Class(class) => {
                if let Some(identifier) = &class.id {
                    self.push_simple_declaration(
                        identifier.name.to_string(),
                        class.span,
                        identifier.span.start,
                        DeclarationKind::Class,
                        class.declare,
                        true,
                    );
                }
            }
            AstKind::TSEnumDeclaration(enumeration) => self.push_simple_declaration(
                enumeration.id.name.to_string(),
                enumeration.span,
                enumeration.id.span.start,
                DeclarationKind::Enum,
                enumeration.declare,
                true,
            ),
            AstKind::VariableDeclarator(declarator) => {
                self.collect_variable_type(declarator);
                if !matches!(
                    declarator.init,
                    Some(
                        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                    )
                ) {
                    for identifier in declarator.id.get_binding_identifiers() {
                        self.push_simple_declaration(
                            identifier.name.to_string(),
                            declarator.span,
                            identifier.span.start,
                            DeclarationKind::Variable,
                            false,
                            declarator.init.is_some(),
                        );
                    }
                }
            }
            AstKind::CallExpression(call) => self.collect_call(call),
            AstKind::StaticMemberExpression(member) => self.collect_member(member),
            AstKind::AwaitExpression(await_expression) => {
                let span = self.span(await_expression.span);
                if let Some(index) = self.active_function_metrics.last().copied() {
                    self.function_metrics[index].awaits.push(span);
                }
            }
            _ if is_loop(kind) => self.loop_stack.push(self.span(kind.span())),
            _ => {}
        }
        if is_runtime_node(kind) {
            let span = kind.span();
            for line in self.lines.line(span.start)
                ..=self.lines.line(span.end.saturating_sub(1).max(span.start))
            {
                self.facts.runtime_lines.insert(line);
            }
        }
        self.node_stack.push(kind);
    }

    fn leave_node(&mut self, kind: AstKind<'a>) {
        self.node_stack.pop();
        if is_branch(kind)
            && is_control(kind)
            && let Some(index) = self.active_function_metrics.last().copied()
        {
            let metric = &mut self.function_metrics[index];
            metric.control_depth = metric.control_depth.saturating_sub(1);
        }
        if is_loop(kind) {
            self.loop_stack.pop();
        }
        if matches!(kind, AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)) {
            self.leave_callback();
            self.scopes.pop();
            self.active_function_metrics.pop();
        }
    }
}

fn narrates_attached_statement(comment: &str, source: &str, attached_to: usize) -> bool {
    let comment_tokens = alphabetic_tokens(comment)
        .into_iter()
        .filter(|token| !matches!(token.as_str(), "the" | "a" | "an" | "to" | "and" | "or"))
        .collect::<BTreeSet<_>>();
    if comment_tokens.len() < 2 {
        return false;
    }
    let statement =
        source.get(attached_to..).unwrap_or_default().lines().next().unwrap_or_default();
    let statement_tokens = alphabetic_tokens(statement).into_iter().collect::<BTreeSet<_>>();
    comment_tokens.is_subset(&statement_tokens)
}

fn alphabetic_tokens(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|token| token.chars().any(char::is_alphabetic))
        .map(str::to_ascii_lowercase)
        .collect()
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/__tests__/")
        || lower.contains("/test/")
        || lower.contains("/tests/")
        || [".test.", ".spec.", "_test.", "_spec."].iter().any(|needle| lower.contains(needle))
}

fn is_generated_path(path: &str, source: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".d.ts")
        || lower.contains(".generated.")
        || lower.contains(".gen.")
        || lower.contains("/generated/")
        || lower.contains("/__generated__/")
        || source.lines().take(5).any(|line| {
            let line = line.to_ascii_lowercase();
            line.contains("@generated")
                || line.contains("code generated")
                || line.contains("do not edit")
        })
}

fn classify_return_shape(expression: &Expression<'_>) -> ReturnShape {
    match expression {
        Expression::NullLiteral(_) => ReturnShape::Null,
        Expression::BooleanLiteral(value) if value.value => ReturnShape::True,
        Expression::BooleanLiteral(_) => ReturnShape::False,
        Expression::StringLiteral(value) if value.value.is_empty() => ReturnShape::EmptyString,
        Expression::ArrayExpression(value) if value.elements.is_empty() => ReturnShape::EmptyArray,
        Expression::ObjectExpression(value) if value.properties.is_empty() => {
            ReturnShape::EmptyObject
        }
        Expression::Identifier(value) if value.name == "undefined" => ReturnShape::Undefined,
        _ => ReturnShape::Other,
    }
}

fn normalize_type_text(type_text: &str) -> String {
    type_text.trim().trim_start_matches(':').trim().to_string()
}

fn type_text_nullable(type_text: &str) -> bool {
    type_text.split('|').any(|part| matches!(part.trim(), "null" | "undefined" | "void"))
}

fn primitive_type(type_text: &str) -> Option<String> {
    ["string", "number", "boolean", "bigint", "symbol", "unknown", "any"]
        .into_iter()
        .find(|primitive| type_text.trim() == *primitive)
        .map(str::to_string)
}

fn parsed_primitive_type(type_text: &str) -> Resolution<String> {
    let normalized = normalize_type_text(type_text);
    ["string", "number", "boolean", "bigint", "symbol", "unknown", "any"]
        .into_iter()
        .find(|primitive| normalized == *primitive)
        .map_or_else(
            || Resolution::Unknown {
                reason: format!("field annotation is not one exact primitive: {normalized}"),
            },
            |primitive| Resolution::Resolved(primitive.to_string()),
        )
}

fn nonnull_initializer_kind(expression: &Expression<'_>) -> Option<NonNullProofKind> {
    match expression {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::ArrayExpression(_)
        | Expression::ObjectExpression(_)
        | Expression::TemplateLiteral(_)
        | Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::ClassExpression(_) => Some(NonNullProofKind::LiteralInitializer),
        Expression::NewExpression(_) => Some(NonNullProofKind::NewInitializer),
        Expression::CallExpression(call)
            if callee_path(&call.callee).last().is_some_and(|name| {
                matches!(name.as_str(), "zParse" | "parse" | "assert" | "invariant")
            }) =>
        {
            Some(NonNullProofKind::ValidatorCall)
        }
        Expression::ParenthesizedExpression(value) => nonnull_initializer_kind(&value.expression),
        Expression::TSAsExpression(value) => nonnull_initializer_kind(&value.expression),
        Expression::TSSatisfiesExpression(value) => nonnull_initializer_kind(&value.expression),
        Expression::TSNonNullExpression(_) => Some(NonNullProofKind::AssertionCall),
        _ => None,
    }
}

const fn statement_contains_function(statement: &Statement<'_>) -> bool {
    matches!(statement, Statement::FunctionDeclaration(_) | Statement::ClassDeclaration(_))
}

fn callee_path(expression: &Expression<'_>) -> Vec<String> {
    match expression {
        Expression::Identifier(identifier) => vec![identifier.name.to_string()],
        Expression::StaticMemberExpression(member) => {
            let mut path = callee_path(&member.object);
            if !path.is_empty() {
                path.push(member.property.name.to_string());
            }
            path
        }
        Expression::ComputedMemberExpression(member) => {
            let mut path = callee_path(&member.object);
            if let Expression::StringLiteral(property) = &member.expression
                && !path.is_empty()
            {
                path.push(property.value.to_string());
            }
            path
        }
        Expression::CallExpression(call) => callee_path(&call.callee),
        Expression::ChainExpression(chain) => match &chain.expression {
            oxc::ast::ast::ChainElement::CallExpression(call) => callee_path(&call.callee),
            oxc::ast::ast::ChainElement::TSNonNullExpression(value) => {
                callee_path(&value.expression)
            }
            oxc::ast::ast::ChainElement::ComputedMemberExpression(member) => {
                let mut path = callee_path(&member.object);
                if let Expression::StringLiteral(property) = &member.expression
                    && !path.is_empty()
                {
                    path.push(property.value.to_string());
                }
                path
            }
            oxc::ast::ast::ChainElement::StaticMemberExpression(member) => {
                let mut path = callee_path(&member.object);
                if !path.is_empty() {
                    path.push(member.property.name.to_string());
                }
                path
            }
            oxc::ast::ast::ChainElement::PrivateFieldExpression(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn boolean_condition(expression: &Expression<'_>) -> Option<ConstantCondition> {
    match expression {
        Expression::BooleanLiteral(value) if value.value => Some(ConstantCondition::AlwaysTrue),
        Expression::BooleanLiteral(_) => Some(ConstantCondition::AlwaysFalse),
        Expression::ParenthesizedExpression(value) => boolean_condition(&value.expression),
        _ => None,
    }
}

fn direct_expression_identifier(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        _ => None,
    }
}

fn transform_operations_for_call(call: &CallExpression<'_>) -> Vec<TransformOp> {
    let mut operations = Vec::new();
    if let Some(name) = callee_path(&call.callee).last() {
        let lower = name.to_ascii_lowercase();
        let operation = if lower.contains("normal")
            || lower.contains("sanit")
            || lower.contains("trim")
        {
            Some(TransformOp::Normalize)
        } else if lower.contains("default") || lower.starts_with("fallback") {
            Some(TransformOp::Default)
        } else if lower.contains("adapt") || lower.starts_with("to") || lower.starts_with("from") {
            Some(TransformOp::Adapter)
        } else if lower.contains("cast") || lower.starts_with("as") {
            Some(TransformOp::Cast)
        } else {
            None
        };
        if let Some(operation) = operation {
            operations.push(operation);
        }
    }
    for argument in &call.arguments {
        if let Some(expression) = argument.as_expression() {
            collect_transform_operations(expression, &mut operations);
        }
    }
    operations
}

fn collect_transform_operations(expression: &Expression<'_>, operations: &mut Vec<TransformOp>) {
    match expression {
        Expression::CallExpression(call) => operations.extend(transform_operations_for_call(call)),
        Expression::TSAsExpression(value) => {
            operations.push(TransformOp::Cast);
            collect_transform_operations(&value.expression, operations);
        }
        Expression::TSSatisfiesExpression(value) => {
            collect_transform_operations(&value.expression, operations);
        }
        Expression::TSTypeAssertion(value) => {
            operations.push(TransformOp::Cast);
            collect_transform_operations(&value.expression, operations);
        }
        _ => {}
    }
}

fn expression_root_name(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        Expression::StaticMemberExpression(member) => expression_root_name(&member.object),
        _ => None,
    }
}

fn callback_expression<'a>(call: &'a CallExpression<'a>) -> Option<&'a Expression<'a>> {
    call.arguments.iter().filter_map(Argument::as_expression).find(|expression| {
        matches!(
            expression,
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
        )
    })
}

fn registration_name(call: &CallExpression<'_>, source: &str) -> Option<String> {
    call.arguments.first().and_then(Argument::as_expression).and_then(|expression| match expression
    {
        Expression::StringLiteral(value) => Some(value.value.to_string()),
        Expression::TemplateLiteral(value) if value.expressions.is_empty() => Some(
            source
                .get(value.span.start as usize + 1..value.span.end.saturating_sub(1) as usize)
                .unwrap_or_default()
                .to_string(),
        ),
        _ => None,
    })
}

fn framework_for_registration(
    path: &[String],
    imports: &[ImportFact],
    conventional_test_path: bool,
) -> TestFramework {
    let root = path.first().map_or("", String::as_str);
    let source = imports
        .iter()
        .find(|import| import.local.as_deref() == Some(root))
        .map(|import| import.source.as_str());
    match source {
        Some("node:test") => TestFramework::NodeTest,
        Some("jsr:@std/testing/bdd") => TestFramework::Deno,
        Some("vitest" | "@jest/globals" | "bun:test") => TestFramework::JestLike,
        _ if conventional_test_path && matches!(root, "test" | "it" | "specify") => {
            TestFramework::JestLike
        }
        _ => TestFramework::Unknown,
    }
}

fn test_registration(
    call: &CallExpression<'_>,
    path: &[String],
    source: &str,
) -> Option<(TestMode, Option<String>, Option<Span>)> {
    let root = path.first()?.as_str();
    let mode = match (root, path.get(1).map(String::as_str)) {
        ("xit" | "xtest", _) => TestMode::DisabledAlias,
        ("test" | "it" | "specify", Some("skip")) => TestMode::Skip,
        ("test" | "it" | "specify", Some("todo")) => TestMode::Todo,
        ("test" | "it" | "specify", Some("only")) => TestMode::Only,
        ("test" | "it" | "specify", Some("each")) => TestMode::Parameterized,
        ("test" | "it" | "specify", Some("prop")) => TestMode::Property,
        ("test" | "it" | "specify", _) => TestMode::Run,
        _ => return None,
    };
    let name = registration_name(call, source);
    let callback = call.arguments.iter().filter_map(|argument| argument.as_expression()).find_map(
        |expression| match expression {
            Expression::ArrowFunctionExpression(value) => Some(value.span),
            Expression::FunctionExpression(value) => Some(value.span),
            _ => None,
        },
    );
    Some((mode, name, callback))
}

struct ShapeDetails {
    shape: ExpressionShape,
    literals: Vec<String>,
    node_count: u16,
    has_mutation: bool,
    has_snapshot: bool,
    has_unknown_dynamic_call: bool,
}

#[allow(clippy::struct_excessive_bools)] // Independent completeness/shape observations.
struct ShapeVisitor<'a, 's> {
    semantic: &'s Semantic<'a>,
    path: &'s str,
    source: &'s str,
    canonical: String,
    literals: Vec<String>,
    node_count: u16,
    complexity: u16,
    call_chain: Vec<String>,
    referenced_symbols: BTreeSet<SymbolKey>,
    resolution_complete: bool,
    has_mutation: bool,
    has_snapshot: bool,
    has_unknown_dynamic_call: bool,
}

impl<'a> Visit<'a> for ShapeVisitor<'a, '_> {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        if kind.is_type() {
            return;
        }
        self.node_count = self.node_count.saturating_add(1);
        self.canonical.push('(');
        self.canonical.push_str(&format!("{:?}", kind.ty()));
        match kind {
            AstKind::StringLiteral(literal) => self.record_literal("str", literal.span),
            AstKind::NumericLiteral(literal) => self.record_literal("num", literal.span),
            AstKind::BooleanLiteral(literal) => self.record_literal("bool", literal.span),
            AstKind::NullLiteral(literal) => self.record_literal("null", literal.span),
            AstKind::BigIntLiteral(literal) => self.record_literal("bigint", literal.span),
            AstKind::RegExpLiteral(literal) => self.record_literal("regexp", literal.span),
            AstKind::TemplateElement(element) => self.record_literal("template", element.span),
            AstKind::IdentifierReference(identifier) => {
                self.canonical.push(':');
                self.canonical.push_str(identifier.name.as_str());
                if let Some(reference_id) = identifier.reference_id.get() {
                    let reference = self.semantic.scoping().get_reference(reference_id);
                    if let Some(symbol_id) = reference.symbol_id() {
                        self.referenced_symbols.insert(SymbolKey {
                            path: self.path.to_string(),
                            declaration_start: self.semantic.scoping().symbol_span(symbol_id).start,
                            name: self.semantic.scoping().symbol_name(symbol_id).to_string(),
                        });
                    } else if !is_known_global(identifier.name.as_str()) {
                        self.resolution_complete = false;
                    }
                } else {
                    self.resolution_complete = false;
                }
            }
            AstKind::IdentifierName(identifier) => {
                self.canonical.push(':');
                self.canonical.push_str(identifier.name.as_str());
            }
            AstKind::BindingIdentifier(identifier) => {
                self.canonical.push(':');
                self.canonical.push_str(identifier.name.as_str());
            }
            AstKind::BinaryExpression(expression) => {
                self.canonical.push_str(&format!(":{:?}", expression.operator));
                self.bump_complexity();
            }
            AstKind::LogicalExpression(expression) => {
                self.canonical.push_str(&format!(":{:?}", expression.operator));
                self.bump_complexity();
            }
            AstKind::UnaryExpression(expression) => {
                self.canonical.push_str(&format!(":{:?}", expression.operator));
                self.bump_complexity();
            }
            AstKind::AssignmentExpression(expression) => {
                self.canonical.push_str(&format!(":{:?}", expression.operator));
                self.has_mutation = true;
                self.bump_complexity();
            }
            AstKind::UpdateExpression(expression) => {
                self.canonical.push_str(&format!(":{:?}", expression.operator));
                self.has_mutation = true;
                self.bump_complexity();
            }
            AstKind::CallExpression(call) => {
                let path = callee_path(&call.callee);
                if path.is_empty() || path.first().is_some_and(|name| name == "eval") {
                    self.has_unknown_dynamic_call = true;
                }
                if path.last().is_some_and(|name| is_snapshot_matcher(name)) {
                    self.has_snapshot = true;
                }
                if !path.is_empty() {
                    self.call_chain.push(path.join("."));
                }
                self.bump_complexity();
            }
            AstKind::ImportExpression(_) => {
                self.has_unknown_dynamic_call = true;
                self.bump_complexity();
            }
            AstKind::NewExpression(_)
            | AstKind::StaticMemberExpression(_)
            | AstKind::ComputedMemberExpression(_)
            | AstKind::PrivateFieldExpression(_)
            | AstKind::ConditionalExpression(_)
            | AstKind::ArrayExpression(_)
            | AstKind::ObjectExpression(_)
            | AstKind::TemplateLiteral(_) => self.bump_complexity(),
            _ => {}
        }
    }

    fn leave_node(&mut self, kind: AstKind<'a>) {
        if !kind.is_type() {
            self.canonical.push(')');
        }
    }
}

impl ShapeVisitor<'_, '_> {
    fn record_literal(&mut self, kind: &str, span: Span) {
        self.canonical.push(':');
        self.canonical.push_str(kind);
        self.literals.push(
            self.source.get(span.start as usize..span.end as usize).unwrap_or_default().to_string(),
        );
    }

    const fn bump_complexity(&mut self) {
        self.complexity = self.complexity.saturating_add(1);
    }

    fn finish(mut self) -> ShapeDetails {
        self.call_chain.sort();
        self.call_chain.dedup();
        ShapeDetails {
            shape: ExpressionShape {
                canonical: self.canonical,
                complexity: self.complexity,
                call_chain: self.call_chain,
                referenced_symbols: self.referenced_symbols.into_iter().collect(),
                resolution_complete: self.resolution_complete,
            },
            literals: self.literals,
            node_count: self.node_count,
            has_mutation: self.has_mutation,
            has_snapshot: self.has_snapshot,
            has_unknown_dynamic_call: self.has_unknown_dynamic_call,
        }
    }
}

fn expression_shape_details<'a>(
    expression: &Expression<'a>,
    semantic: &Semantic<'a>,
    path: &str,
    source: &str,
) -> ShapeDetails {
    let mut visitor = ShapeVisitor {
        semantic,
        path,
        source,
        canonical: String::new(),
        literals: Vec::new(),
        node_count: 0,
        complexity: 0,
        call_chain: Vec::new(),
        referenced_symbols: BTreeSet::new(),
        resolution_complete: true,
        has_mutation: false,
        has_snapshot: false,
        has_unknown_dynamic_call: false,
    };
    visitor.visit_expression(expression);
    visitor.finish()
}

fn shape_expression<'a>(
    expression: &Expression<'a>,
    semantic: &Semantic<'a>,
    path: &str,
    source: &str,
) -> ExpressionShape {
    expression_shape_details(expression, semantic, path, source).shape
}

struct TestBodyDetails {
    body_span: Span,
    shape: TestBodyShape,
    has_snapshot: bool,
    has_unknown_dynamic_call: bool,
}

fn test_body_details<'a>(
    callback: &Expression<'a>,
    semantic: &Semantic<'a>,
    path: &str,
    source: &str,
) -> Option<TestBodyDetails> {
    let (body_span, statement_count, details) = match callback {
        Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => {
                let mut visitor = new_shape_visitor(semantic, path, source);
                visitor.visit_function_body(body);
                (body.span, body.statements.len(), visitor.finish())
            }
            body => {
                let expression = body.as_expression()?;
                (expression.span(), 1, expression_shape_details(expression, semantic, path, source))
            }
        },
        Expression::FunctionExpression(function) => {
            let body = function.body.as_ref()?;
            let mut visitor = new_shape_visitor(semantic, path, source);
            visitor.visit_function_body(body);
            (body.span, body.statements.len(), visitor.finish())
        }
        _ => return None,
    };
    Some(TestBodyDetails {
        body_span,
        shape: TestBodyShape {
            canonical: details.shape.canonical,
            literal_vector: details.literals,
            statement_count: statement_count.try_into().unwrap_or(u16::MAX),
            node_count: details.node_count,
        },
        has_snapshot: details.has_snapshot,
        has_unknown_dynamic_call: details.has_unknown_dynamic_call,
    })
}

const fn new_shape_visitor<'a, 's>(
    semantic: &'s Semantic<'a>,
    path: &'s str,
    source: &'s str,
) -> ShapeVisitor<'a, 's> {
    ShapeVisitor {
        semantic,
        path,
        source,
        canonical: String::new(),
        literals: Vec::new(),
        node_count: 0,
        complexity: 0,
        call_chain: Vec::new(),
        referenced_symbols: BTreeSet::new(),
        resolution_complete: true,
        has_mutation: false,
        has_snapshot: false,
        has_unknown_dynamic_call: false,
    }
}

fn single_return_expression<'a>(body: &'a FunctionBody<'a>) -> Option<&'a Expression<'a>> {
    match body.statements.as_slice() {
        [Statement::ReturnStatement(statement)] => statement.argument.as_ref(),
        _ => None,
    }
}

struct ParsedAssertion<'a> {
    api: AssertionApi,
    root: String,
    matcher: String,
    negated: bool,
    async_modifier: Option<String>,
    actual: Option<&'a Expression<'a>>,
    expected: Option<&'a Expression<'a>>,
}

fn parse_assertion<'a>(call: &'a CallExpression<'a>) -> Option<ParsedAssertion<'a>> {
    let path = callee_path(&call.callee);
    let root = path.first()?.as_str();
    if root == "expect" && path.len() >= 2 {
        let matcher = path.last()?.as_str();
        if matches!(matcher, "not" | "resolves" | "rejects") {
            return None;
        }
        let expect_call = find_root_call(&call.callee, "expect")?;
        return Some(ParsedAssertion {
            api: AssertionApi::Expect,
            root: root.to_string(),
            matcher: matcher.to_string(),
            negated: path.iter().any(|part| part == "not"),
            async_modifier: path
                .iter()
                .find(|part| matches!(part.as_str(), "resolves" | "rejects"))
                .cloned(),
            actual: expect_call.arguments.first().and_then(Argument::as_expression),
            expected: call.arguments.first().and_then(Argument::as_expression),
        });
    }
    if root == "assert" {
        let matcher = path.get(1).map_or("ok", String::as_str);
        return Some(ParsedAssertion {
            api: AssertionApi::NodeAssert,
            root: root.to_string(),
            matcher: matcher.to_string(),
            negated: matcher.to_ascii_lowercase().starts_with("not"),
            async_modifier: (matcher == "rejects").then(|| "rejects".to_string()),
            actual: call.arguments.first().and_then(Argument::as_expression),
            expected: call.arguments.get(1).and_then(Argument::as_expression),
        });
    }
    None
}

fn find_root_call<'a>(
    expression: &'a Expression<'a>,
    root: &str,
) -> Option<&'a CallExpression<'a>> {
    match expression {
        Expression::CallExpression(call) if matches!(&call.callee, Expression::Identifier(identifier) if identifier.name == root) => {
            Some(call)
        }
        Expression::CallExpression(call) => find_root_call(&call.callee, root),
        Expression::StaticMemberExpression(member) => find_root_call(&member.object, root),
        Expression::ComputedMemberExpression(member) => find_root_call(&member.object, root),
        Expression::ChainExpression(chain) => match &chain.expression {
            oxc::ast::ast::ChainElement::CallExpression(call) => find_root_call(&call.callee, root),
            oxc::ast::ast::ChainElement::TSNonNullExpression(value) => {
                find_root_call(&value.expression, root)
            }
            oxc::ast::ast::ChainElement::ComputedMemberExpression(member) => {
                find_root_call(&member.object, root)
            }
            oxc::ast::ast::ChainElement::StaticMemberExpression(member) => {
                find_root_call(&member.object, root)
            }
            oxc::ast::ast::ChainElement::PrivateFieldExpression(member) => {
                find_root_call(&member.object, root)
            }
        },
        _ => None,
    }
}

fn assertion_api_resolution(
    root: &str,
    imports: &[ImportFact],
    conventional_test_path: bool,
) -> Resolution<String> {
    if let Some(import) = imports.iter().find(|import| import.local.as_deref() == Some(root)) {
        if matches!(
            import.source.as_str(),
            "vitest" | "@jest/globals" | "node:assert" | "node:assert/strict" | "chai"
        ) {
            return Resolution::Resolved(import.source.clone());
        }
        return Resolution::Unknown {
            reason: format!("{root} resolves to unsupported assertion source {}", import.source),
        };
    }
    if conventional_test_path && matches!(root, "expect" | "assert") {
        Resolution::Resolved("conventional test global".to_string())
    } else {
        Resolution::Unknown { reason: "assertion API is not exactly resolved".to_string() }
    }
}

fn assertion_boundary(matcher: &str, expected: Option<&Expression<'_>>) -> AssertionBoundary {
    let lower = matcher.to_ascii_lowercase();
    if lower.contains("throw") || lower.contains("reject") {
        return AssertionBoundary::Error;
    }
    if matches!(lower.as_str(), "tobenull" | "tobeundefined" | "benull" | "beundefined") {
        return AssertionBoundary::Nullish;
    }
    if matches!(lower.as_str(), "tobetruthy" | "tobefalsy" | "istrue" | "isfalse") {
        return AssertionBoundary::BooleanEdge;
    }
    match expected {
        Some(Expression::NullLiteral(_)) => AssertionBoundary::Nullish,
        Some(Expression::Identifier(identifier))
            if matches!(identifier.name.as_str(), "undefined" | "nil") =>
        {
            AssertionBoundary::Nullish
        }
        Some(Expression::BooleanLiteral(_)) => AssertionBoundary::BooleanEdge,
        Some(Expression::NumericLiteral(number)) if number.value == 0.0 => {
            AssertionBoundary::NumericEdge
        }
        Some(Expression::StringLiteral(string)) if string.value.is_empty() => {
            AssertionBoundary::Empty
        }
        Some(Expression::ArrayExpression(array)) if array.elements.is_empty() => {
            AssertionBoundary::Empty
        }
        Some(Expression::ObjectExpression(object)) if object.properties.is_empty() => {
            AssertionBoundary::Empty
        }
        _ => AssertionBoundary::None,
    }
}

fn direct_call_key(expression: &Expression<'_>) -> Option<u32> {
    match expression {
        Expression::CallExpression(call) => Some(call.span.start),
        Expression::AwaitExpression(value) => direct_call_key(&value.argument),
        Expression::ParenthesizedExpression(value) => direct_call_key(&value.expression),
        Expression::TSAsExpression(value) => direct_call_key(&value.expression),
        Expression::TSNonNullExpression(value) => direct_call_key(&value.expression),
        _ => None,
    }
}

fn is_snapshot_matcher(name: &str) -> bool {
    matches!(
        name,
        "toMatchSnapshot"
            | "toMatchInlineSnapshot"
            | "toThrowErrorMatchingSnapshot"
            | "toThrowErrorMatchingInlineSnapshot"
            | "toMatchImageSnapshot"
    )
}

fn mock_kind(path: &[String]) -> Option<MockKind> {
    let name = path.last()?.as_str();
    match name {
        "fn" => Some(MockKind::Factory),
        "mock" | "mocked" => Some(MockKind::Module),
        "spyOn" => Some(MockKind::Spy),
        "stub" | "replaceProperty" => Some(MockKind::Stub),
        "mockImplementation"
        | "mockImplementationOnce"
        | "mockReturnValue"
        | "mockReturnValueOnce"
        | "mockResolvedValue"
        | "mockResolvedValueOnce"
        | "mockRejectedValue"
        | "mockRejectedValueOnce"
        | "callsFake"
        | "returns"
        | "resolves"
        | "rejects"
        | "throws"
        | "onCall" => Some(MockKind::Behavior),
        "restoreAllMocks" | "resetAllMocks" | "clearAllMocks" | "restore" => {
            Some(MockKind::Restore)
        }
        _ => None,
    }
}

fn is_known_global(name: &str) -> bool {
    matches!(
        name,
        "undefined"
            | "console"
            | "Promise"
            | "Math"
            | "JSON"
            | "Object"
            | "Array"
            | "String"
            | "Number"
            | "Boolean"
            | "Date"
            | "RegExp"
            | "Error"
            | "expect"
            | "assert"
            | "test"
            | "it"
            | "describe"
            | "suite"
            | "beforeEach"
            | "afterEach"
            | "beforeAll"
            | "afterAll"
            | "vi"
            | "jest"
    )
}

const fn is_loop(kind: AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
    )
}

const fn is_branch(kind: AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::IfStatement(_)
            | AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::CatchClause(_)
            | AstKind::ConditionalExpression(_)
            | AstKind::LogicalExpression(_)
            | AstKind::SwitchCase(_)
    )
}

const fn is_control(kind: AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::IfStatement(_)
            | AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::CatchClause(_)
            | AstKind::ConditionalExpression(_)
    )
}

const fn is_runtime_node(kind: AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::ExpressionStatement(_)
            | AstKind::ReturnStatement(_)
            | AstKind::ThrowStatement(_)
            | AstKind::IfStatement(_)
            | AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::SwitchStatement(_)
            | AstKind::VariableDeclarator(_)
            | AstKind::CallExpression(_)
            | AstKind::NewExpression(_)
            | AstKind::AssignmentExpression(_)
    )
}

#[cfg(test)]
mod tests {
    use oxc::{allocator::Allocator, parser::Parser, semantic::SemanticBuilder, span::SourceType};

    use super::*;

    #[test]
    fn comments_are_taken_from_parser_trivia_not_strings() {
        let source = "const text = \"@ts-ignore\"; // @ts-ignore\nconst value = input as any;";
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        let semantic =
            SemanticBuilder::new().with_build_nodes(true).build(&parsed.program).semantic;
        let facts =
            collect_facts(&parsed.program, &semantic, source, "a.ts", &[], &BTreeSet::new(), true);
        assert_eq!(facts.comments.len(), 1);
        assert_eq!(facts.comments[0].directive, Some(SuppressionDirectiveKind::TsIgnore));
        assert_eq!(facts.casts.len(), 1);
        assert_eq!(facts.casts[0].kind, CastKind::AsAny);
    }
}
