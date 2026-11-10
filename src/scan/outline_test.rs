use super::*;

fn symbol(name: &str, kind: SymbolKind, detail: bool, children: Vec<Symbol>) -> Symbol {
    Symbol { children, detail, ..Symbol::new(name.to_string(), kind, 1, 1) }
}

fn paths(outline: &FileOutline) -> Vec<String> {
    let mut paths = Vec::new();
    outline.walk(&mut |_, path| paths.push(path.to_string()));
    paths
}

fn sample() -> FileOutline {
    FileOutline {
        symbols: vec![
            symbol(
                "make",
                SymbolKind::Const,
                false,
                vec![
                    symbol("db", SymbolKind::Const, false, Vec::new()),
                    symbol(
                        "record",
                        SymbolKind::Function,
                        false,
                        vec![
                            symbol("tokens", SymbolKind::Const, true, Vec::new()),
                            symbol(
                                "handlers",
                                SymbolKind::Const,
                                true,
                                vec![
                                    symbol("limit", SymbolKind::Property, true, Vec::new()),
                                    symbol("run", SymbolKind::Method, false, Vec::new()),
                                ],
                            ),
                        ],
                    ),
                ],
            ),
            symbol(
                "Color",
                SymbolKind::Enum,
                false,
                vec![symbol("Red", SymbolKind::EnumMember, true, Vec::new())],
            ),
        ],
        reexports: vec![Reexport {
            from: "./a".into(),
            names: vec!["one as uno".into(), "two".into()],
            all: false,
            namespace: None,
            type_only: false,
        }],
        imports: vec!["effect/Effect".into()],
    }
}

#[test]
fn walk_visits_depth_first_with_dotted_paths() {
    assert_eq!(
        paths(&sample()),
        [
            "make",
            "make.db",
            "make.record",
            "make.record.tokens",
            "make.record.handlers",
            "make.record.handlers.limit",
            "make.record.handlers.run",
            "Color",
            "Color.Red",
        ]
    );
}

#[test]
fn structure_drops_detail_but_keeps_owners_of_structural_symbols() {
    let structure = sample().view(OutlineView::default());

    assert_eq!(
        paths(&structure),
        [
            "make",
            "make.db",
            "make.record",
            "make.record.handlers",
            "make.record.handlers.run",
            "Color"
        ]
    );
    assert_eq!(structure.reexports, sample().reexports);
    assert_eq!(structure.imports, [] as [String; 0]);
}

#[test]
fn reexport_names_are_the_public_ones() {
    let named = &sample().reexports[0];
    assert_eq!(named.exported_names(), ["uno", "two"]);

    let star = Reexport { all: true, names: Vec::new(), ..named.clone() };
    assert_eq!(star.exported_names(), ["*"]);

    let namespace = Reexport { namespace: Some("ns".into()), ..star };
    assert_eq!(namespace.exported_names(), ["ns"]);
}

#[test]
fn serializes_only_meaningful_fields() {
    let mut exported = symbol("main", SymbolKind::Function, false, Vec::new());
    exported.exported = true;
    exported.exported_as = vec!["default".into()];
    exported.via = Some("Effect.fn".into());
    exported.variable = Some(SymbolKind::Const);
    exported.function_position = Some((1, 1));

    assert_eq!(
        serde_json::to_value(&exported).unwrap(),
        serde_json::json!({
            "name": "main", "kind": "function", "via": "Effect.fn", "exported": true,
            "exportedAs": ["default"], "line": 1, "endLine": 1
        })
    );
    assert_eq!(
        serde_json::to_value(symbol("Red", SymbolKind::EnumMember, true, Vec::new())).unwrap(),
        serde_json::json!({ "name": "Red", "kind": "enumMember", "line": 1, "endLine": 1 })
    );
}

#[test]
fn views_select_signatures_and_the_exported_surface() {
    let mut run = symbol("run", SymbolKind::Function, false, Vec::new());
    run.signature = Some("(id: string)".into());
    run.exported = true;
    let mut outline = sample();
    outline.symbols.push(run);

    let default = outline.view(OutlineView::default());
    assert_eq!(default.symbols[2].signature, None);

    let signatures = outline.view(OutlineView { signatures: true, ..OutlineView::default() });
    assert_eq!(signatures.symbols[2].signature.as_deref(), Some("(id: string)"));

    let exported = outline.view(OutlineView { exported: true, ..OutlineView::default() });
    assert_eq!(paths(&exported), ["run"]);
    assert_eq!(exported.reexports, outline.reexports);

    let deep = outline.view(OutlineView { deep: true, ..OutlineView::default() });
    assert_eq!(paths(&deep).len(), 10);
    assert_eq!(deep.imports, ["effect/Effect"]);
}
