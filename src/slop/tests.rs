use std::fs;

use tempfile::tempdir;

use crate::scan::types::FunctionKindsFilter;
use crate::scan::typescript::parse::process_file_with_similarity;
use crate::slop::types::{
    AssertionApi, AssertionBoundary, CallRole, CastKind, ImportSpecifierKind, MockKind,
    NonNullProofKind, Resolution, ScopeKey, SetupKind,
};
use crate::slop::{build_project_facts, collect_project_files, detectors};

#[test]
fn public_registry_covers_every_non_diff_kind_once_in_schema_order() {
    let registry = detectors(&crate::slop::types::SlopOptions::default());
    let kinds = registry.iter().map(|detector| detector.kind()).collect::<Vec<_>>();
    let expected = crate::slop::types::SlopKind::ALL
        .into_iter()
        .filter(|kind| !kind.is_diff_only())
        .collect::<Vec<_>>();
    assert_eq!(kinds, expected);
    let unique = kinds.iter().copied().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), kinds.len());
}

#[test]
fn shared_parse_collects_neutral_exact_and_contextual_facts() {
    let root = tempdir().unwrap();
    let file = root.path().join("facts.test.tsx");
    fs::write(
        &file,
        r#"
import type { User } from "./models";
// @ts-ignore
export interface Draft { id: string; note?: string | null }
export type Alias = { enabled?: boolean; count: number };
async function save(value: User) { await persist(value); }
function example(input: unknown) {
  try { save(input as any); } catch (error) { console.error(error); }
  if (false) { return input!; }
}
test.skip("does not run", () => { expect(example("x")).toBeTruthy(); });
"#,
    )
    .unwrap();
    fs::write(root.path().join("models.ts"), "export interface User { id: string }\n").unwrap();

    let canonical = root.path().canonicalize().unwrap();
    let (_, parsed) =
        process_file_with_similarity(&file, &canonical, FunctionKindsFilter::All).unwrap();
    let facts = parsed.slop;

    assert!(facts.analysis_complete);
    assert!(facts.is_test);
    assert_eq!(facts.comments.len(), 1);
    assert_eq!(facts.casts.iter().filter(|cast| cast.kind == CastKind::AsAny).count(), 1);
    assert_eq!(facts.casts.iter().filter(|cast| cast.kind == CastKind::NonNull).count(), 1);
    assert_eq!(facts.catches.len(), 1);
    assert!(facts.catches[0].can_fall_through);
    assert!(facts.imports.iter().any(|import| {
        import.kind == ImportSpecifierKind::Named
            && import.type_only
            && import.local.as_deref() == Some("User")
    }));
    assert_eq!(facts.models.len(), 2);
    assert!(facts.models.iter().any(|model| model.fields.iter().any(|field| field.nullable)));
    assert!(facts.declarations.iter().any(|declaration| declaration.key.name == "save"));
    assert!(
        facts
            .async_calls
            .iter()
            .any(|call| call.callee_path.last().is_some_and(|name| name == "save"))
    );
    assert_eq!(facts.tests.len(), 1);
    assert!(!facts.tests[0].assertion_spans.is_empty());
    assert_eq!(facts.branches.len(), 1);
    assert!(facts.branches[0].unreachable_span.is_some());
    assert!(facts.runtime_lines.iter().all(|line| *line > 0));
    assert!(facts.comments.iter().all(|comment| comment.span.path == "facts.test.tsx"));
}

#[test]
fn project_indexes_resolve_modules_exports_aliases_packages_and_calls() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/lib")).unwrap();
    fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    fs::write(
        root.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@lib/*":["src/lib/*"]}}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib/index.ts"),
        "export async function load() { return 1; }\nexport interface Shape { id: string }\n",
    )
    .unwrap();
    fs::write(root.path().join("src/local.ts"), "export const local = 1;\n").unwrap();
    fs::write(
        root.path().join("src/main.ts"),
        r#"
import { local } from "./local";
import { load } from "@lib/index";
import type { External } from "pkg";
import { missing } from "./local";
import "./absent";
load();
console.log(local);
"#,
    )
    .unwrap();
    fs::write(root.path().join("node_modules/pkg/package.json"), r#"{"types":"index.d.ts"}"#)
        .unwrap();
    fs::write(root.path().join("node_modules/pkg/index.d.ts"), "export interface External {}\n")
        .unwrap();

    let canonical = root.path().canonicalize().unwrap();
    let files = collect_project_files(&canonical).unwrap();
    let project = build_project_facts(&canonical, files).unwrap();

    let local = project
        .imports
        .iter()
        .find(|(key, _)| key.source == "./local" && key.imported.as_deref() == Some("local"))
        .map(|(_, resolution)| resolution)
        .unwrap();
    assert!(matches!(local.module, Resolution::Resolved(ref path) if path == "src/local.ts"));
    assert!(matches!(local.export, Resolution::Resolved(_)));

    let alias = project
        .imports
        .iter()
        .find(|(key, _)| key.source == "@lib/index")
        .map(|(_, resolution)| resolution)
        .unwrap();
    assert!(matches!(alias.module, Resolution::Resolved(ref path) if path == "src/lib/index.ts"));

    let package = project
        .imports
        .iter()
        .find(|(key, _)| key.source == "pkg")
        .map(|(_, resolution)| resolution)
        .unwrap();
    assert!(
        matches!(package.module, Resolution::Resolved(ref path) if path == "node_modules/pkg/index.d.ts")
    );
    assert!(matches!(package.export, Resolution::Unknown { .. }));

    let missing_export = project
        .imports
        .iter()
        .find(|(key, _)| key.imported.as_deref() == Some("missing"))
        .map(|(_, resolution)| resolution)
        .unwrap();
    assert!(matches!(missing_export.export, Resolution::Missing { .. }));
    let missing_module = project
        .imports
        .iter()
        .find(|(key, _)| key.source == "./absent")
        .map(|(_, resolution)| resolution)
        .unwrap();
    assert!(matches!(missing_module.module, Resolution::Missing { .. }));

    assert!(project.call_targets.values().any(|target| {
        matches!(target, Resolution::Resolved(symbol) if symbol.path == "src/lib/index.ts" && symbol.name == "load")
    }));
    assert!(project.symbol_uses.iter().any(|(symbol, uses)| {
        symbol.path == "src/lib/index.ts"
            && symbol.name == "load"
            && uses.iter().any(|span| span.path == "src/main.ts")
    }));
    assert!(!project.coverage.unsupported_offline_proofs.is_empty());
}

#[test]
fn project_indexes_are_deterministic_across_input_order() {
    let root = tempdir().unwrap();
    fs::write(
        root.path().join("a.ts"),
        "import type { B } from './b';\nexport interface A { id: string; enabled?: boolean; peer?: B }\nexport function alpha() { return 1; }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("b.ts"),
        "export interface B { enabled?: boolean; id: string }\nimport { alpha } from './a';\nalpha();\n",
    )
    .unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let files = collect_project_files(&canonical).unwrap();
    let mut reversed = files.clone();
    reversed.reverse();
    let first = build_project_facts(&canonical, files).unwrap();
    let second = build_project_facts(&canonical, reversed).unwrap();

    assert_eq!(first.files, second.files);
    assert_eq!(first.symbol_uses, second.symbol_uses);
    assert_eq!(first.imports, second.imports);
    assert_eq!(first.call_targets, second.call_targets);
    assert_eq!(first.function_groups, second.function_groups);
    assert_eq!(first.type_groups, second.type_groups);
    assert_eq!(first.model_field_types, second.model_field_types);
    assert_eq!(first.symbol_types, second.symbol_types);
    assert_eq!(first.suites, second.suites);
    assert_eq!(first.setups, second.setups);
    assert_eq!(first.tests, second.tests);
    assert_eq!(first.assertions, second.assertions);
    assert_eq!(first.mocks, second.mocks);
    assert_eq!(first.call_arguments, second.call_arguments);
    assert_eq!(first.call_roles, second.call_roles);
    assert_eq!(first.production_expressions, second.production_expressions);
    assert_eq!(first.coverage, second.coverage);
}

#[test]
#[allow(clippy::too_many_lines)] // End-to-end fixture asserts one coherent flow contract.
fn extracts_nonnull_proofs_primitives_and_if_complexity() {
    let root = tempdir().unwrap();
    let file = root.path().join("context.ts");
    fs::write(
        &file,
        r"
interface Shape { text: string; count: number; enabled: boolean; maybe?: string | null }
function inspect(value: string, optional?: string | null) {
  if (value == null) return;
  if (value.length > 0) {}
  if (value.length > 1) {}
  if (value.length > 2) {}
  if (value.length > 3) {}
  return value;
}
function parsed(raw: unknown) {
  const value: string = schema.parse(raw);
  if (value == null) return;
  return value;
}
function asserted(value: string | null) {
  assert(value);
  if (value == null) return;
  return value;
}
",
    )
    .unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let (_, parsed) =
        process_file_with_similarity(&file, &canonical, FunctionKindsFilter::All).unwrap();
    let facts = parsed.slop;

    let shape = facts.models.iter().find(|model| model.key.name == "Shape").unwrap();
    assert!(shape.fields.iter().all(|field| field.primitive.is_none()));
    let primitive_fields = facts
        .model_field_types
        .iter()
        .filter(|field| field.model == shape.key)
        .map(|field| (field.field.as_str(), &field.primitive))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert!(matches!(primitive_fields["text"], Resolution::Resolved(value) if value == "string"));
    assert!(matches!(primitive_fields["count"], Resolution::Resolved(value) if value == "number"));
    assert!(
        matches!(primitive_fields["enabled"], Resolution::Resolved(value) if value == "boolean")
    );
    assert!(matches!(primitive_fields["maybe"], Resolution::Unknown { .. }));

    let inspect =
        facts.declarations.iter().find(|declaration| declaration.key.name == "inspect").unwrap();
    assert_eq!(inspect.branch_complexity, 6);
    let required = facts
        .symbol_types
        .values()
        .find(|symbol| {
            symbol.key.name == "value" && symbol.key.declaration_start < inspect.span.end_byte
        })
        .unwrap();
    assert!(matches!(
        required.proven_nonnull,
        Resolution::Resolved(ref proof) if proof.kind == NonNullProofKind::RequiredParameter
    ));
    assert!(matches!(required.scope, ScopeKey::Function(ref key) if key == &inspect.key));
    let optional =
        facts.symbol_types.values().find(|symbol| symbol.key.name == "optional").unwrap();
    assert!(optional.nullable);
    assert!(matches!(optional.proven_nonnull, Resolution::Unknown { .. }));

    let parsed_function =
        facts.declarations.iter().find(|declaration| declaration.key.name == "parsed").unwrap();
    let validator = facts
        .symbol_types
        .values()
        .find(|symbol| {
            symbol.key.name == "value"
                && matches!(symbol.scope, ScopeKey::Function(ref key) if key == &parsed_function.key)
        })
        .unwrap();
    assert!(matches!(
        validator.proven_nonnull,
        Resolution::Resolved(ref proof) if proof.kind == NonNullProofKind::ValidatorCall
    ));
    let parsed_guard = facts
        .guards
        .iter()
        .find(|guard| {
            guard.guarded_symbol.as_deref() == Some("value")
                && matches!(guard.scope, ScopeKey::Function(ref key) if key == &parsed_function.key)
        })
        .unwrap();
    assert_eq!(parsed_guard.scope, validator.scope);

    let asserted =
        facts.declarations.iter().find(|declaration| declaration.key.name == "asserted").unwrap();
    let asserted_value = facts
        .symbol_types
        .values()
        .find(|symbol| {
            symbol.key.name == "value"
                && matches!(symbol.scope, ScopeKey::Function(ref key) if key == &asserted.key)
        })
        .unwrap();
    let asserted_guard = facts
        .guards
        .iter()
        .find(|guard| {
            guard.guarded_symbol.as_deref() == Some("value")
                && matches!(guard.scope, ScopeKey::Function(ref key) if key == &asserted.key)
        })
        .unwrap();
    assert!(matches!(
        asserted_value.proven_nonnull,
        Resolution::Resolved(ref proof)
            if proof.kind == NonNullProofKind::AssertionCall
                && proof.effective_after_byte < asserted_guard.span.start_byte
    ));
}

#[test]
#[allow(clippy::too_many_lines)] // End-to-end fixture covers interdependent test indexes.
fn extracts_and_indexes_exact_test_semantics_without_reparsing() {
    let root = tempdir().unwrap();
    fs::write(
        root.path().join("production.ts"),
        "export function normalize(input: string) { return input.trim().toLowerCase(); }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("behavior.test.ts"),
        r#"
import { describe, test, beforeEach, expect, vi } from "vitest";
import { normalize } from "./production";
describe("normalize", () => {
  beforeEach(() => { vi.fn().mockReturnValue("ready"); });
  test("first", async () => {
    const result = normalize(" A ");
    await expect(normalize(" A ")).resolves.not.toEqual(result);
  });
  test("second", async () => {
    const result = normalize(" B ");
    await expect(normalize(" B ")).resolves.not.toEqual(result);
  });
  test("boundary", () => { expect(null).toBeNull(); });
  test("snapshot", () => { expect(normalize("C")).toMatchSnapshot(); });
});
"#,
    )
    .unwrap();

    let canonical = root.path().canonicalize().unwrap();
    let project =
        build_project_facts(&canonical, collect_project_files(&canonical).unwrap()).unwrap();
    let file = &project.files["behavior.test.ts"];

    assert_eq!(file.suites.len(), 2);
    let explicit_suite = file.suites.iter().find(|suite| suite.id.registration_start > 0).unwrap();
    assert_eq!(explicit_suite.name.as_deref(), Some("normalize"));
    assert_eq!(file.setups.len(), 1);
    assert_eq!(file.setups[0].kind, SetupKind::BeforeEach);
    assert_eq!(file.setups[0].suite, explicit_suite.id);
    let setup_mock = file
        .mocks
        .iter()
        .find(|mock| mock.setup.is_some() && mock.kind == MockKind::Behavior)
        .unwrap();
    assert_eq!(setup_mock.kind, MockKind::Behavior);
    assert_eq!(setup_mock.suite, explicit_suite.id);

    let first = file
        .test_cases
        .iter()
        .find(|test| {
            file.tests.iter().any(|legacy| {
                legacy.name.as_deref() == Some("first")
                    && legacy.call_span.start_byte == test.id.registration_start
            })
        })
        .unwrap();
    let second = file
        .test_cases
        .iter()
        .find(|test| {
            file.tests.iter().any(|legacy| {
                legacy.name.as_deref() == Some("second")
                    && legacy.call_span.start_byte == test.id.registration_start
            })
        })
        .unwrap();
    assert_eq!(first.suite, explicit_suite.id);
    assert_eq!(first.body_shape.statement_count, 2);
    assert!(first.body_shape.node_count >= 6);
    assert_eq!(first.body_shape.canonical, second.body_shape.canonical);
    assert_ne!(first.body_shape.literal_vector, second.body_shape.literal_vector);

    let async_assertion = file
        .assertions
        .iter()
        .find(|assertion| assertion.async_modifier.as_deref() == Some("resolves"))
        .unwrap();
    assert_eq!(async_assertion.api, AssertionApi::Expect);
    assert!(async_assertion.negated);
    assert_eq!(async_assertion.matcher, "toEqual");
    assert!(async_assertion.actual.is_some());
    assert!(async_assertion.expected.is_some());
    assert!(matches!(
        async_assertion.invokes,
        Resolution::Resolved(ref symbol)
            if symbol.path == "production.ts" && symbol.name == "normalize"
    ));
    assert!(async_assertion.actual.as_ref().is_some_and(|shape| !shape.canonical.contains(" A ")));

    let boundary =
        file.assertions.iter().find(|assertion| assertion.matcher == "toBeNull").unwrap();
    assert_eq!(boundary.boundary, AssertionBoundary::Nullish);
    assert!(file.test_cases.iter().any(|test| test.has_snapshot));

    let production = project
        .production_expressions
        .values()
        .find(|production| production.owner.name == "normalize")
        .unwrap();
    assert!(production.eligible);
    assert!(production.returned.complexity >= 3);
    assert_eq!(production.parameter_names, ["input"]);
    assert!(project.call_arguments.iter().any(|(key, arguments)| {
        key.path == "behavior.test.ts"
            && arguments.arguments.iter().any(|shape| !shape.canonical.is_empty())
    }));
    assert!(
        project
            .call_roles
            .iter()
            .any(|(key, role)| { key.path == "behavior.test.ts" && *role == CallRole::Sut })
    );
    assert!(
        project
            .call_roles
            .iter()
            .any(|(key, role)| { key.path == "behavior.test.ts" && *role == CallRole::Assertion })
    );
    assert_eq!(project.tests.len(), file.test_cases.len());
    assert_eq!(project.assertions.len(), file.assertions.len());
}

#[test]
fn parse_incomplete_files_are_recorded_without_absence_claims() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("bad.ts"), "export function broken( {\n").unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let project =
        build_project_facts(&canonical, collect_project_files(&canonical).unwrap()).unwrap();
    assert!(project.coverage.parse_incomplete_files.contains("bad.ts"));
    assert!(!project.files["bad.ts"].analysis_complete);
}
