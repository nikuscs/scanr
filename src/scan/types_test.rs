use super::*;

#[test]
fn function_kinds_filter_includes_variants() {
    assert!(FunctionKindsFilter::All.includes(FunctionKind::Getter));
    assert!(FunctionKindsFilter::Top.includes(FunctionKind::Declaration));
    assert!(!FunctionKindsFilter::Top.includes(FunctionKind::Arrow));
    assert!(FunctionKindsFilter::TopArrow.includes(FunctionKind::Arrow));
    assert!(FunctionKindsFilter::TopArrow.includes(FunctionKind::Declaration));
    assert!(!FunctionKindsFilter::TopArrow.includes(FunctionKind::ClassMethod));
    assert!(FunctionKindsFilter::TopArrowClass.includes(FunctionKind::Declaration));
    assert!(FunctionKindsFilter::TopArrowClass.includes(FunctionKind::Arrow));
    assert!(FunctionKindsFilter::TopArrowClass.includes(FunctionKind::ClassMethod));
    assert!(FunctionKindsFilter::TopArrowClass.includes(FunctionKind::Constructor));
    assert!(FunctionKindsFilter::TopArrowClass.includes(FunctionKind::Getter));
    assert!(FunctionKindsFilter::TopArrowClass.includes(FunctionKind::Setter));
    assert!(!FunctionKindsFilter::TopArrowClass.includes(FunctionKind::ObjectMethod));
    assert!(!FunctionKindsFilter::TopArrowClass.includes(FunctionKind::Expression));
}

#[test]
fn function_kind_labels_and_codes_cover_all() {
    let kinds = [
        FunctionKind::Declaration,
        FunctionKind::Arrow,
        FunctionKind::Expression,
        FunctionKind::ClassMethod,
        FunctionKind::ObjectMethod,
        FunctionKind::Getter,
        FunctionKind::Setter,
        FunctionKind::Constructor,
    ];
    for k in kinds {
        let code = k.code();
        let label = k.label();
        assert!(!code.is_empty());
        assert!(!label.is_empty());
    }
}

#[test]
fn line_index_bounds() {
    let src = "a\n\nccc\n";
    let li = LineIndex::new(src);
    assert_eq!(li.line(0), 1);
    assert_eq!(li.col(0), 1);
    assert!(li.line(2) >= 1);
    assert!(li.line(3) >= 1);
    let _ = li.col(6);
}

#[test]
fn binding_kind_strings_and_export_codes_are_covered() {
    let kinds = [
        BindingKind::Var,
        BindingKind::Let,
        BindingKind::Const,
        BindingKind::Param,
        BindingKind::Function,
        BindingKind::Class,
        BindingKind::Import,
        BindingKind::Catch,
        BindingKind::Enum,
    ];
    for k in kinds {
        assert!(!k.as_str().is_empty());
    }
    assert_eq!(EXPORT_NAMED, 1);
    assert_eq!(EXPORT_DEFAULT, 2);
    assert_eq!(EXPORT_REEXPORT, 3);
}
