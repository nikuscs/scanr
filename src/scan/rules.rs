use std::collections::HashSet;

use crate::scan::types::{BindingKind, FileIndex, FunctionKind, Violation};

pub trait Rule: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, index: &FileIndex) -> Option<Violation>;
}

pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(NoUnusedBindings),
        Box::new(OneExportedFunctionPerFile { path_prefix: None }),
        Box::new(MaxFunctionsPerFile { max: 20 }),
        Box::new(HoistableNestedFunction { include_test_files: false }),
    ]
}

pub fn run_rules(enabled: &[String], index: &mut FileIndex) {
    run_rules_with_test_files(enabled, index, false);
}

pub fn run_rules_with_test_files(
    enabled: &[String],
    index: &mut FileIndex,
    include_test_files: bool,
) {
    let enabled_set: HashSet<&str> = enabled.iter().map(String::as_str).collect();
    let mut rules = all_rules();
    if include_test_files {
        rules.pop();
        rules.push(Box::new(HoistableNestedFunction { include_test_files: true }));
    }

    for rule in
        rules.into_iter().filter(|r| enabled_set.is_empty() || enabled_set.contains(r.name()))
    {
        if let Some(violation) = rule.check(index) {
            index.violations.push(violation);
        }
    }
}

struct NoUnusedBindings;

impl Rule for NoUnusedBindings {
    fn name(&self) -> &'static str {
        "no_unused_bindings"
    }

    fn check(&self, index: &FileIndex) -> Option<Violation> {
        let unused: Vec<String> = index
            .bindings
            .iter()
            .filter(|b| {
                b.refs == 0 && !b.name.starts_with('_') && !matches!(b.kind, BindingKind::Import)
            })
            .map(|b| b.name.clone())
            .collect();

        if unused.is_empty() {
            return None;
        }

        Some(Violation { rule: self.name().to_string(), count: unused.len(), details: unused })
    }
}

struct OneExportedFunctionPerFile {
    path_prefix: Option<String>,
}

impl Rule for OneExportedFunctionPerFile {
    fn name(&self) -> &'static str {
        "one_exported_function_per_file"
    }

    fn check(&self, index: &FileIndex) -> Option<Violation> {
        if let Some(prefix) = &self.path_prefix
            && !index.path.starts_with(prefix.as_str())
        {
            return None;
        }

        let exported_fns: Vec<String> =
            index.functions.iter().filter(|f| f.exported).filter_map(|f| f.name.clone()).collect();

        if exported_fns.len() <= 1 {
            return None;
        }

        Some(Violation {
            rule: self.name().to_string(),
            count: exported_fns.len(),
            details: exported_fns,
        })
    }
}

struct HoistableNestedFunction {
    include_test_files: bool,
}

impl Rule for HoistableNestedFunction {
    fn name(&self) -> &'static str {
        "hoistable_nested_function"
    }

    fn check(&self, index: &FileIndex) -> Option<Violation> {
        if !self.include_test_files && is_test_path(&index.path) {
            return None;
        }

        let functions: Vec<String> = index
            .functions
            .iter()
            .filter(|function| function.parent.is_some() && function.captures.is_empty())
            .filter(|function| {
                matches!(
                    function.kind,
                    FunctionKind::Declaration | FunctionKind::Arrow | FunctionKind::Expression
                )
            })
            .filter_map(|function| {
                function.name.as_ref().map(|name| {
                    function
                        .parent
                        .as_ref()
                        .map_or_else(|| name.clone(), |parent| format!("{parent}.{name}"))
                })
            })
            .collect();

        if functions.is_empty() {
            return None;
        }

        Some(Violation {
            rule: self.name().to_string(),
            count: functions.len(),
            details: functions,
        })
    }
}

fn is_test_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.split('/').any(|part| matches!(part, "test" | "tests" | "__tests__"))
        || normalized.contains(".test.")
        || normalized.contains(".spec.")
        || normalized.ends_with("_test.ts")
        || normalized.ends_with("_test.tsx")
        || normalized.ends_with("_test.js")
        || normalized.ends_with("_test.jsx")
}

struct MaxFunctionsPerFile {
    max: usize,
}

impl Rule for MaxFunctionsPerFile {
    fn name(&self) -> &'static str {
        "max_functions_per_file"
    }

    fn check(&self, index: &FileIndex) -> Option<Violation> {
        let count = index.functions.len();
        if count <= self.max {
            return None;
        }

        let names: Vec<String> = index.functions.iter().filter_map(|f| f.name.clone()).collect();

        Some(Violation { rule: self.name().to_string(), count, details: names })
    }
}

#[cfg(test)]
#[path = "rules_test.rs"]
mod tests;
