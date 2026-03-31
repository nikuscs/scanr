use super::*;
use crate::scan::types::{FunctionKind, FunctionKindsFilter, LineIndex};
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
    let semantic = SemanticBuilder::new().build(&ret.program).semantic;
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

    let binding_names: std::collections::BTreeSet<_> =
        result.bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(binding_names.contains("arr"));
    assert!(binding_names.contains("K"));
    assert!(binding_names.contains("e"));

    assert!(lines.line(0) >= 1);
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
    let semantic = SemanticBuilder::new().build(&ret.program).semantic;

    let top_only = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::Top);
    assert!(!top_only.functions.iter().any(|f| f.name.as_deref() == Some("ignored")));

    let all = extract_file(&ret.program, &semantic, src, FunctionKindsFilter::All);
    assert!(all.functions.iter().any(|f| f.kind == FunctionKind::Expression));
    assert!(all.functions.iter().any(|f| f.name.as_deref() == Some("namedExpr")));
    assert!(all.functions.iter().any(|f| f.name.as_deref() == Some("plainFn")));
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
    let semantic = SemanticBuilder::new().build(&ret.program).semantic;
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
