use super::*;
use crate::scan::types::{FunctionKind, FunctionKindsFilter, LineIndex, TypeDeclKind};
use oxc::allocator::Allocator;
use oxc::parser::{ParseOptions, Parser};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;

#[test]
fn param_extraction_variants() {
    let allocator = Allocator::default();
    let src = "function f(a,{b},[c], d = 1){}";
    let st = SourceType::default().with_module(false).with_script(true);
    let ret = Parser::new(&allocator, src, st).with_options(ParseOptions::default()).parse();
    if let oxc::ast::ast::Statement::FunctionDeclaration(fd) = &ret.program.body[0] {
        let names = extract_param_names(&fd.params);
        assert_eq!(names, vec!["a", "{...}", "[...]", "d"]);
    } else {
        panic!("unexpected AST");
    }
}

#[test]
fn covers_exports_classes_object_methods_and_bindings() {
    let allocator = Allocator::default();
    let src = r"
            export { a1, a2 };
            export function foo() {}
            export default function defFn() {}
            export default function() {}
            export default class NamedCls {}
            export const arr = () => {};
            class K { constructor(){} get g(){ return 1 } set s(v){} m(){} }
            const obj = { get og(){ return 1 }, set os(v){}, method(){ const inner = () => {}; }, key: 1 };
            enum E { A }
            import { z } from 'm';
            try { throw new Error() } catch (e) { const c = 1; }
        ";
    let st = SourceType::ts().with_module(true);
    let ret = Parser::new(&allocator, src, st).with_options(ParseOptions::default()).parse();
    let semantic = SemanticBuilder::new().with_build_nodes(true).build(&ret.program).semantic;
    let lines = LineIndex::new(src);
    let result = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::All);

    let names: std::collections::BTreeSet<_> =
        result.functions.iter().filter_map(|f| f.name.clone()).collect();
    assert!(names.contains("foo"));
    assert!(names.contains("defFn"));
    assert!(names.contains("arr"));
    assert!(names.contains("g"));
    assert!(names.contains("s"));
    assert!(names.contains("m"));
    assert!(names.contains("method"));
    assert!(names.contains("inner"));

    let export_names: std::collections::BTreeSet<_> =
        result.exports.iter().map(|e| e.name.as_str()).collect();
    assert!(export_names.contains("default"));
    assert!(export_names.contains("foo"));
    assert!(export_names.contains("NamedCls"));
    assert!(result.classes.iter().any(|class| class.name == "NamedCls" && class.exported));
    assert!(result.classes.iter().any(|class| class.name == "K" && !class.exported));

    let binding_names: std::collections::BTreeSet<_> =
        result.bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(binding_names.contains("arr"));
    assert!(binding_names.contains("K"));
    assert!(binding_names.contains("e"));

    assert!(lines.line(0) >= 1);
}

#[test]
fn jsx_is_attributed_to_the_innermost_function() {
    let allocator = Allocator::default();
    let src = r"
        function Card() {
            const renderLabel = () => <span>Label</span>;
            return <section>{renderLabel()}</section>;
        }
        function Utility() { return 1; }
    ";
    let st = SourceType::from_path(std::path::Path::new("view.tsx")).unwrap();
    let ret = Parser::new(&allocator, src, st).with_options(ParseOptions::default()).parse();
    let semantic = SemanticBuilder::new().with_build_nodes(true).build(&ret.program).semantic;
    let result = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::All);

    let card =
        result.functions.iter().find(|function| function.name.as_deref() == Some("Card")).unwrap();
    let label = result
        .functions
        .iter()
        .find(|function| function.name.as_deref() == Some("renderLabel"))
        .unwrap();
    let utility = result
        .functions
        .iter()
        .find(|function| function.name.as_deref() == Some("Utility"))
        .unwrap();
    assert!(card.has_jsx());
    assert!(label.has_jsx());
    assert!(!utility.has_jsx());
}

#[test]
fn type_declarations_are_collected_with_export_status() {
    let allocator = Allocator::default();
    let src = r"
        export interface Range { start: number }
        export type Confidence = 'low' | 'high'
        interface Internal { x: number }
        type Local = string
    ";
    let st = SourceType::ts().with_module(true);
    let ret = Parser::new(&allocator, src, st).with_options(ParseOptions::default()).parse();
    let semantic = SemanticBuilder::new().with_build_nodes(true).build(&ret.program).semantic;
    let result = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::All);

    let by_name: std::collections::BTreeMap<_, _> =
        result.types.iter().map(|t| (t.name.as_str(), t)).collect();
    assert_eq!(by_name.len(), 4);
    assert_eq!(by_name["Range"].kind, TypeDeclKind::Interface);
    assert!(by_name["Range"].exported);
    assert_eq!(by_name["Confidence"].kind, TypeDeclKind::TypeAlias);
    assert!(by_name["Confidence"].exported);
    assert_eq!(by_name["Internal"].kind, TypeDeclKind::Interface);
    assert!(!by_name["Internal"].exported);
    assert_eq!(by_name["Local"].kind, TypeDeclKind::TypeAlias);
    assert!(!by_name["Local"].exported);

    let export_names: std::collections::BTreeSet<_> =
        result.exports.iter().map(|e| e.name.as_str()).collect();
    assert!(export_names.contains("Range"));
    assert!(export_names.contains("Confidence"));
    assert!(!export_names.contains("Internal"));
    assert!(!export_names.contains("Local"));
}

#[test]
fn filter_and_misc_ast_branches_are_covered() {
    let allocator = Allocator::default();
    let src = r"
        export interface I { x: number }
        const ignored = () => 1;
        const expr = function namedExpr() { return 1; };
        const obj = { plain: function plainFn() { return 1; } };
        declare function declared(a: number): void;
    ";
    let st = SourceType::ts().with_module(true);
    let ret = Parser::new(&allocator, src, st).with_options(ParseOptions::default()).parse();
    let semantic = SemanticBuilder::new().with_build_nodes(true).build(&ret.program).semantic;

    let top_only = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::Top);
    assert!(!top_only.functions.iter().any(|f| f.name.as_deref() == Some("ignored")));

    let all = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::All);
    assert!(all.functions.iter().any(|f| f.kind == FunctionKind::Expression));
    assert!(all.functions.iter().any(|f| f.name.as_deref() == Some("namedExpr")));
    assert!(all.functions.iter().any(|f| f.name.as_deref() == Some("plainFn")));
}

#[test]
fn nested_functions_record_parents_and_enclosing_captures() {
    let allocator = Allocator::default();
    let src = r"
        const moduleValue = 1;
        function Component() {
            const local = 2;
            function hoistable() { return moduleValue; }
            const captured = () => local + moduleValue;
        }
    ";
    let st = SourceType::ts().with_module(true);
    let ret = Parser::new(&allocator, src, st).with_options(ParseOptions::default()).parse();
    let semantic = SemanticBuilder::new().with_build_nodes(true).build(&ret.program).semantic;
    let result = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::All);

    let hoistable = result
        .functions
        .iter()
        .find(|function| function.name.as_deref() == Some("hoistable"))
        .unwrap();
    assert_eq!(hoistable.parent.as_deref(), Some("Component"));
    assert!(hoistable.captures.is_empty());

    let captured = result
        .functions
        .iter()
        .find(|function| function.name.as_deref() == Some("captured"))
        .unwrap();
    assert_eq!(captured.parent.as_deref(), Some("Component"));
    assert_eq!(captured.captures, vec!["local"]);
}

#[test]
fn classifies_only_trivial_low_value_shapes() {
    let allocator = Allocator::default();
    let src = r"
        function empty() {}
        const constant = () => 1;
        function wrapper(value: string) { return save(value); }
        function directProperty(value: { id: string }) { return value.id; }
        function chained(value: string) { return save(value).trim(); }
        function mapped(value: { kind: string }) { return RANK[value.kind]; }
        function complex(value: number) { const next = value + 1; return next; }
    ";
    let st = SourceType::ts().with_module(true);
    let ret = Parser::new(&allocator, src, st).with_options(ParseOptions::default()).parse();
    let semantic = SemanticBuilder::new().with_build_nodes(true).build(&ret.program).semantic;
    let result = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::All);
    let functions: std::collections::BTreeMap<_, _> = result
        .functions
        .iter()
        .filter_map(|function| {
            function.name.as_deref().map(|name| (name, function.low_value_reason.as_deref()))
        })
        .collect();

    assert_eq!(functions["empty"], Some("empty"));
    assert_eq!(functions["constant"], Some("constant_return"));
    assert_eq!(functions["wrapper"], Some("thin_wrapper"));
    assert_eq!(functions["directProperty"], Some("property_return"));
    assert_eq!(functions["chained"], None);
    assert_eq!(functions["mapped"], None);
    assert_eq!(functions["complex"], None);
}

#[test]
fn default_export_misc_and_assignment_pattern_fallback() {
    let allocator = Allocator::default();
    let src = r"
        export default 123;
        export class C {}
        export enum E { A }
        const obj = { set sx(v) {}, get gx() { return 1; } };
        function p({a} = {}) {}
    ";
    let st = SourceType::ts().with_module(true);
    let ret = Parser::new(&allocator, src, st).with_options(ParseOptions::default()).parse();
    let semantic = SemanticBuilder::new().with_build_nodes(true).build(&ret.program).semantic;
    let result = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::All);

    let export_names: std::collections::BTreeSet<_> =
        result.exports.iter().map(|e| e.name.as_str()).collect();
    assert!(export_names.contains("default"));
    assert!(export_names.contains("C"));
    assert!(export_names.contains("E"));

    assert!(!result.functions.is_empty());

    if let oxc::ast::ast::Statement::FunctionDeclaration(fd) = &ret.program.body[4] {
        let names = extract_param_names(&fd.params);
        assert_eq!(names, vec!["{...}"]);
    } else {
        panic!("expected function declaration");
    }
}
