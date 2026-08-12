use std::collections::BTreeSet;

use oxc::ast::{AstKind, ast::Expression};
use oxc::ast_visit::Visit;

#[derive(Debug, Clone, Default)]
pub struct HealthAstMetrics {
    pub dependencies: BTreeSet<String>,
    pub hook_calls: usize,
    pub state_calls: usize,
    pub effect_calls: usize,
    pub max_branch_complexity: usize,
    pub max_control_nesting: usize,
}

#[derive(Debug, Default)]
struct FunctionComplexity {
    branches: usize,
    control_depth: usize,
    max_control_depth: usize,
}

#[derive(Debug, Default)]
struct HealthVisitor {
    metrics: HealthAstMetrics,
    functions: Vec<FunctionComplexity>,
}

pub fn analyze_program(program: &oxc::ast::ast::Program<'_>) -> HealthAstMetrics {
    let mut visitor = HealthVisitor::default();
    visitor.visit_program(program);
    visitor.metrics
}

impl<'a> Visit<'a> for HealthVisitor {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        match kind {
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
                self.functions.push(FunctionComplexity::default());
            }
            AstKind::ImportDeclaration(import) => {
                self.metrics.dependencies.insert(import.source.value.to_string());
            }
            AstKind::ExportAllDeclaration(export) => {
                self.metrics.dependencies.insert(export.source.value.to_string());
            }
            AstKind::CallExpression(call) => {
                if let Expression::Identifier(identifier) = &call.callee {
                    let name = identifier.name.as_str();
                    if is_hook_name(name) {
                        self.metrics.hook_calls += 1;
                    }
                    if matches!(name, "useState" | "useReducer") {
                        self.metrics.state_calls += 1;
                    }
                    if matches!(name, "useEffect" | "useLayoutEffect" | "useInsertionEffect") {
                        self.metrics.effect_calls += 1;
                    }
                }
            }
            _ if is_branch(kind) => {
                if let Some(function) = self.functions.last_mut() {
                    function.branches += 1;
                    if is_control_nesting(kind) {
                        function.control_depth += 1;
                        function.max_control_depth =
                            function.max_control_depth.max(function.control_depth);
                    }
                }
            }
            _ => {}
        }
    }

    fn leave_node(&mut self, kind: AstKind<'a>) {
        if is_branch(kind)
            && is_control_nesting(kind)
            && let Some(function) = self.functions.last_mut()
        {
            function.control_depth = function.control_depth.saturating_sub(1);
        }
        if matches!(kind, AstKind::Function(_) | AstKind::ArrowFunctionExpression(_))
            && let Some(function) = self.functions.pop()
        {
            self.metrics.max_branch_complexity =
                self.metrics.max_branch_complexity.max(function.branches + 1);
            self.metrics.max_control_nesting =
                self.metrics.max_control_nesting.max(function.max_control_depth);
        }
    }
}

fn is_hook_name(name: &str) -> bool {
    name.strip_prefix("use")
        .is_some_and(|suffix| suffix.chars().next().is_some_and(char::is_uppercase))
}

const fn is_branch(kind: AstKind<'_>) -> bool {
    match kind {
        AstKind::IfStatement(_)
        | AstKind::ForStatement(_)
        | AstKind::ForInStatement(_)
        | AstKind::ForOfStatement(_)
        | AstKind::WhileStatement(_)
        | AstKind::DoWhileStatement(_)
        | AstKind::CatchClause(_)
        | AstKind::ConditionalExpression(_)
        | AstKind::LogicalExpression(_) => true,
        AstKind::SwitchCase(case) => case.test.is_some(),
        _ => false,
    }
}

const fn is_control_nesting(kind: AstKind<'_>) -> bool {
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

#[cfg(test)]
mod tests {
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    use super::*;

    #[test]
    fn collects_module_react_and_complexity_metrics() {
        let allocator = Allocator::default();
        let source = r#"
import { useEffect, useState } from "react";
import { save } from "./api";
export function Example() {
  const [value] = useState(0);
  useEffect(() => { if (value) save(value); }, [value]);
  if (value > 1) return value > 2 ? 2 : 1;
  return 0;
}
"#;
        let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let metrics = analyze_program(&parsed.program);
        assert_eq!(
            metrics.dependencies,
            BTreeSet::from(["./api".to_string(), "react".to_string()])
        );
        assert_eq!(metrics.hook_calls, 2);
        assert_eq!(metrics.state_calls, 1);
        assert_eq!(metrics.effect_calls, 1);
        assert!(metrics.max_branch_complexity >= 3);
        assert!(metrics.max_control_nesting >= 1);
    }
}
