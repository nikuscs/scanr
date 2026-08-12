//! Medium-confidence contextual slop detectors.
//!
//! This module owns the eight intent-sensitive detectors from Phase 5 of the
//! slop-detection plan. Every detector emits **medium** confidence, requires two
//! independent signals before emitting, iterates only over ordered
//! (`BTreeMap`/sorted `Vec`) foundation facts, and never inspects source text or
//! the oxc AST directly. Role and body classification is consumed verbatim from
//! the shared foundation (`DeclarationFact::role`, `DeclarationFact::body_shape`)
//! — this lane never re-derives it.
//!
//! All eight detectors are live. `RedundantDefense` uses the foundation's
//! `symbol_types` non-null proofs together with guard/cast evidence;
//! `ParallelRepresentation` reads resolved primitives from `model_field_types`; and
//! the complexity-gated detectors rely on the corrected `branch_complexity` (which
//! now counts `if` statements). No detector is a no-op or degraded.

// The public `detectors` registry is consumed by the integration lane that wires it
// into `slop::mod`; until then it is dead to the binary, like the sibling registries.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use crate::commands::dupes::FunctionDuplicateKey;
use crate::scan::types::FunctionRole;
use crate::slop::types::{
    BodyShape, CastKind, DeclarationFact, DeclarationKind, Detector, FileFacts, ModelFact,
    ModelKind, NonNullProofKind, ProjectFacts, Resolution, ScopeKey, SlopConfidence, SlopEvidence,
    SlopFinding, SlopKind, SlopOptions, SourceSpan, SymbolKey, TransformOp, TypeKey,
};

/// The eight contextual detectors owned by this lane.
pub fn detectors(options: &SlopOptions) -> Vec<Box<dyn Detector>> {
    let include_test_files = options.include_test_files;
    vec![
        Box::new(ReinventedHelper { include_test_files }),
        Box::new(RedundantDefense { include_test_files }),
        Box::new(OneUseAbstraction { include_test_files }),
        Box::new(PatchStack { include_test_files }),
        Box::new(SpeculativeModel { include_test_files }),
        Box::new(CommentInversion { include_test_files }),
        Box::new(ParallelRepresentation { include_test_files }),
        Box::new(GenericNameCluster { include_test_files }),
    ]
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// A file is out of scope when it is generated (covers `*.d.ts`), a test file we
/// were not asked to include, or one the parser could not fully analyze.
const fn skipped(file: &FileFacts, include_test_files: bool) -> bool {
    file.is_generated || (!include_test_files && file.is_test) || !file.analysis_complete
}

const fn is_function_kind(kind: DeclarationKind) -> bool {
    matches!(kind, DeclarationKind::Function | DeclarationKind::Method)
}

const fn is_react_role(role: Option<FunctionRole>) -> bool {
    matches!(
        role,
        Some(FunctionRole::ReactComponent | FunctionRole::ReactHook | FunctionRole::ComponentLocal)
    )
}

fn evidence(code: &str, label: &str, span: SourceSpan, detail: &str) -> SlopEvidence {
    SlopEvidence {
        code: code.to_string(),
        label: label.to_string(),
        span,
        detail: detail.to_string(),
    }
}

/// Build a medium-confidence finding. `evidence[0]` is the caller-chosen anchor and
/// stays first; the remaining rows are sorted by `(path, line, byte)` for stable output.
fn finding(
    kind: SlopKind,
    mut rows: Vec<SlopEvidence>,
    explanation: &str,
    action: &str,
) -> SlopFinding {
    if rows.len() > 1 {
        let anchor = rows.remove(0);
        rows.sort_by(|left, right| {
            (&left.span.path, left.span.start_line, left.span.start_byte).cmp(&(
                &right.span.path,
                right.span.start_line,
                right.span.start_byte,
            ))
        });
        rows.insert(0, anchor);
    }
    let span = rows[0].span.clone();
    SlopFinding {
        kind,
        confidence: SlopConfidence::Medium,
        span,
        evidence: rows,
        explanation: explanation.to_string(),
        action: action.to_string(),
    }
}

/// Deterministically order a detector's findings by anchor `(path, line, byte)` then kind.
fn sorted(mut findings: Vec<SlopFinding>) -> Vec<SlopFinding> {
    findings.sort_by(|left, right| {
        (&left.span.path, left.span.start_line, left.span.start_byte, left.kind.cli_name()).cmp(&(
            &right.span.path,
            right.span.start_line,
            right.span.start_byte,
            right.kind.cli_name(),
        ))
    });
    findings
}

fn uses_of<'a>(project: &'a ProjectFacts, key: &SymbolKey) -> &'a [SourceSpan] {
    project.symbol_uses.get(key).map_or(&[][..], Vec::as_slice)
}

// ---------------------------------------------------------------------------
// 1. ReinventedHelper
// ---------------------------------------------------------------------------

/// A local helper structurally duplicates an exported, cross-file-referenced helper
/// that the local file does not import.
struct ReinventedHelper {
    include_test_files: bool,
}

impl ReinventedHelper {
    fn declaration<'a>(
        &self,
        project: &'a ProjectFacts,
        key: &FunctionDuplicateKey,
    ) -> Option<&'a DeclarationFact> {
        let (path, line, name) = key;
        let file = project.files.get(path)?;
        if skipped(file, self.include_test_files) {
            return None;
        }
        file.declarations.iter().find(|decl| {
            is_function_kind(decl.kind) && decl.span.start_line == *line && decl.key.name == *name
        })
    }
}

fn has_cross_file_use(project: &ProjectFacts, decl: &DeclarationFact) -> bool {
    uses_of(project, &decl.key).iter().any(|span| span.path != decl.key.path)
}

fn imports_symbol(project: &ProjectFacts, importer: &str, target: &SymbolKey) -> bool {
    project
        .imports
        .iter()
        .any(|(key, res)| key.importer == importer && res.resolved_symbol.as_ref() == Some(target))
}

impl Detector for ReinventedHelper {
    fn kind(&self) -> SlopKind {
        SlopKind::ReinventedHelper
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for (key, component) in &project.function_groups {
            // Process each connected component exactly once, at its smallest member.
            if component.first() != Some(key) {
                continue;
            }
            let members: Vec<&DeclarationFact> =
                component.iter().filter_map(|member| self.declaration(project, member)).collect();
            let files: BTreeSet<&String> = members.iter().map(|decl| &decl.key.path).collect();
            if files.len() < 2 {
                // Same-file overloads / a single surviving file are not reinvention.
                continue;
            }

            // Signal A: an exported helper referenced from another file exists.
            let exemplar = members
                .iter()
                .filter(|decl| {
                    decl.role == Some(FunctionRole::Helper)
                        && !decl.exported_as.is_empty()
                        && has_cross_file_use(project, decl)
                })
                .min_by(|left, right| left.key.cmp(&right.key));
            let Some(exemplar) = exemplar else { continue };

            // Signal B: a helper in another file reimplements it without importing it.
            for candidate in &members {
                if candidate.key == exemplar.key
                    || candidate.key.path == exemplar.key.path
                    || candidate.role != Some(FunctionRole::Helper)
                    || is_react_role(candidate.role)
                    || !candidate.exported_as.is_empty() && has_cross_file_use(project, candidate)
                {
                    continue;
                }
                if imports_symbol(project, &candidate.key.path, &exemplar.key) {
                    continue;
                }
                findings.push(finding(
                    SlopKind::ReinventedHelper,
                    vec![
                        evidence(
                            &candidate.key.name,
                            "reimplements",
                            candidate.span.clone(),
                            "local helper structurally duplicates an existing shared helper",
                        ),
                        evidence(
                            &exemplar.key.name,
                            "existing helper",
                            exemplar.span.clone(),
                            "exported helper already referenced from other files",
                        ),
                    ],
                    "This local helper repeats an existing shared helper instead of reusing it.",
                    "Import and call the existing helper, then delete this copy.",
                ));
            }
        }
        sorted(findings)
    }
}

// ---------------------------------------------------------------------------
// 2. RedundantDefense
// ---------------------------------------------------------------------------

/// A runtime null-guard or non-null assertion on a value that the same function has
/// already proven non-null (required non-null parameter, literal/`new` initializer,
/// or validator/assertion call).
struct RedundantDefense {
    include_test_files: bool,
}

/// The symbol a guard or non-null cast narrows, plus the span to anchor evidence on.
struct Narrowing<'a> {
    symbol: &'a str,
    scope: &'a ScopeKey,
    span: &'a SourceSpan,
    label: &'static str,
}

const fn scope_function_key(scope: &ScopeKey) -> Option<&SymbolKey> {
    match scope {
        ScopeKey::Function(key) => Some(key),
        _ => None,
    }
}

/// Two scopes name the same function. The foundation stamps a function's
/// `declaration_start` inconsistently across fact kinds (guards vs casts vs
/// `symbol_types`), so scopes are matched by `(path, name)` rather than full key.
fn same_function_scope(left: &ScopeKey, right: &ScopeKey) -> bool {
    match (scope_function_key(left), scope_function_key(right)) {
        (Some(left), Some(right)) => left.path == right.path && left.name == right.name,
        _ => false,
    }
}

/// True when the guard sits at an exported function's parameter boundary — legitimate
/// input validation rather than a redundant guard on a locally proven value.
fn is_entry_boundary(file: &FileFacts, scope: &ScopeKey, proof_kind: NonNullProofKind) -> bool {
    proof_kind == NonNullProofKind::RequiredParameter
        && scope_function_key(scope).is_some_and(|key| {
            file.declarations.iter().any(|decl| {
                is_function_kind(decl.kind)
                    && decl.key.path == key.path
                    && decl.key.name == key.name
                    && !decl.exported_as.is_empty()
            })
        })
}

/// Root identifier referenced at the start of a non-null cast's operand, if any.
fn cast_operand_symbol<'a>(file: &'a FileFacts, operand: &SourceSpan) -> Option<&'a str> {
    file.references
        .iter()
        .filter(|reference| {
            reference.span.start_byte == operand.start_byte
                && reference.span.end_byte == operand.end_byte
        })
        .max_by_key(|reference| reference.span.end_byte)
        .map(|reference| reference.name.as_str())
}

impl Detector for RedundantDefense {
    fn kind(&self) -> SlopKind {
        SlopKind::RedundantDefense
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            // Signal A candidates: null-guards and non-null assertions on a named symbol.
            let mut narrowings: Vec<Narrowing<'_>> = Vec::new();
            for guard in &file.guards {
                if let Some(symbol) = &guard.guarded_symbol {
                    narrowings.push(Narrowing {
                        symbol,
                        scope: &guard.scope,
                        span: &guard.span,
                        label: "redundant guard",
                    });
                }
            }
            for cast in &file.casts {
                if cast.kind != CastKind::NonNull {
                    continue;
                }
                if let Some(symbol) = cast_operand_symbol(file, &cast.operand_span) {
                    narrowings.push(Narrowing {
                        symbol,
                        scope: &cast.scope,
                        span: &cast.span,
                        label: "redundant non-null assertion",
                    });
                }
            }

            for narrowing in narrowings {
                // Signal B: the same symbol is proven non-null in the same function,
                // and the proof takes effect before the narrowing.
                let proven = file.symbol_types.values().find_map(|fact| {
                    if !same_function_scope(&fact.scope, narrowing.scope)
                        || fact.key.name != narrowing.symbol
                    {
                        return None;
                    }
                    match &fact.proven_nonnull {
                        Resolution::Resolved(proof)
                            if proof.effective_after_byte <= narrowing.span.start_byte =>
                        {
                            Some(proof)
                        }
                        _ => None,
                    }
                });
                let Some(proof) = proven else { continue };
                if is_entry_boundary(file, narrowing.scope, proof.kind) {
                    continue;
                }
                findings.push(finding(
                    SlopKind::RedundantDefense,
                    vec![
                        evidence(
                            narrowing.symbol,
                            narrowing.label,
                            narrowing.span.clone(),
                            "guards a value already proven non-null in this function",
                        ),
                        evidence(
                            narrowing.symbol,
                            "non-null proof",
                            proof.span.clone(),
                            "the value is established non-null before the guard",
                        ),
                    ],
                    "This guard re-checks a value the same function already proved non-null.",
                    "Remove the redundant guard and rely on the proven value.",
                ));
            }
        }
        sorted(findings)
    }
}

// ---------------------------------------------------------------------------
// 3. OneUseAbstraction
// ---------------------------------------------------------------------------

/// A pass-through wrapper with a single caller, or an interface with exactly one
/// implementer referenced only from that implementer.
struct OneUseAbstraction {
    include_test_files: bool,
}

impl OneUseAbstraction {
    fn caller_is_test(&self, project: &ProjectFacts, path: &str) -> bool {
        !self.include_test_files && project.files.get(path).is_some_and(|file| file.is_test)
    }

    fn wrapper_findings(&self, project: &ProjectFacts, out: &mut Vec<SlopFinding>) {
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            for decl in &file.declarations {
                // Signal A: the body only forwards to another call.
                if !is_function_kind(decl.kind) || decl.body_shape != BodyShape::PassThroughCall {
                    continue;
                }
                // Exported wrappers may cross a public boundary; stay conservative.
                if !decl.exported_as.is_empty() || is_react_role(decl.role) {
                    continue;
                }
                // Signal B: exactly one caller anywhere in the project.
                let uses = uses_of(project, &decl.key);
                if uses.len() != 1 {
                    continue;
                }
                let caller = &uses[0];
                if self.caller_is_test(project, &caller.path) {
                    continue;
                }
                out.push(finding(
                    SlopKind::OneUseAbstraction,
                    vec![
                        evidence(
                            &decl.key.name,
                            "pass-through wrapper",
                            decl.span.clone(),
                            "wrapper only forwards to another call",
                        ),
                        evidence(
                            &decl.key.name,
                            "only caller",
                            caller.clone(),
                            "single call site in the project",
                        ),
                    ],
                    "This wrapper only forwards to one call and is used in a single place.",
                    "Inline the wrapped call at its one caller and remove the wrapper.",
                ));
            }
        }
    }

    fn interface_findings(&self, project: &ProjectFacts, out: &mut Vec<SlopFinding>) {
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            for model in &file.models {
                if model.kind != ModelKind::Interface {
                    continue;
                }
                // Signal A: exactly one other model extends this interface.
                let implementers: Vec<&ModelFact> = project
                    .files
                    .values()
                    .flat_map(|other| other.models.iter())
                    .filter(|other| other.key != model.key && extends_type(other, &model.key.name))
                    .collect();
                if implementers.len() != 1 {
                    continue;
                }
                let implementer = implementers[0];
                // Signal B: the interface is consumed from exactly one file, and that
                // file is the implementer. Type-only imports of an interface carry no
                // `resolved_symbol` and are not remapped into `symbol_uses`, so
                // single-consumer is measured via the resolved import module + name.
                let importers: BTreeSet<&String> = project
                    .imports
                    .iter()
                    .filter(|(import_key, res)| {
                        import_key.imported.as_deref() == Some(model.key.name.as_str())
                            && matches!(&res.module, Resolution::Resolved(path) if *path == model.key.path)
                    })
                    .map(|(import_key, _)| &import_key.importer)
                    .collect();
                if importers.len() != 1 || !importers.contains(&implementer.key.path) {
                    continue;
                }
                out.push(finding(
                    SlopKind::OneUseAbstraction,
                    vec![
                        evidence(
                            &model.key.name,
                            "single-use interface",
                            model.span.clone(),
                            "interface has exactly one implementer",
                        ),
                        evidence(
                            &implementer.key.name,
                            "only implementer",
                            implementer.span.clone(),
                            "the sole type that extends the interface",
                        ),
                    ],
                    "This interface has one implementer and is referenced only from it.",
                    "Fold the interface into its single implementer.",
                ));
            }
        }
    }
}

fn extends_type(model: &ModelFact, name: &str) -> bool {
    model.extends.iter().any(|clause| {
        clause
            .split(|character: char| !character.is_alphanumeric() && character != '_')
            .any(|token| token == name)
    })
}

impl Detector for OneUseAbstraction {
    fn kind(&self) -> SlopKind {
        SlopKind::OneUseAbstraction
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        self.wrapper_findings(project, &mut findings);
        self.interface_findings(project, &mut findings);
        sorted(findings)
    }
}

// ---------------------------------------------------------------------------
// 4. PatchStack
// ---------------------------------------------------------------------------

/// Three or more consecutive normalize/cast/default/adapter operations on one value,
/// spanning at least two distinct operation categories.
struct PatchStack {
    include_test_files: bool,
}

const fn op_label(op: TransformOp) -> &'static str {
    match op {
        TransformOp::Normalize => "normalize",
        TransformOp::Cast => "cast",
        TransformOp::Default => "default",
        TransformOp::Adapter => "adapter",
    }
}

impl Detector for PatchStack {
    fn kind(&self) -> SlopKind {
        SlopKind::PatchStack
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            // Signal A: chains of >= 3 transform operations.
            let qualifying: Vec<&_> =
                file.transform_chains.iter().filter(|chain| chain.operations.len() >= 3).collect();
            for chain in &qualifying {
                // Signal B: at least two distinct operation categories (pure cast
                // stacks are Phase 4 SuppressionChain territory, not patch stacks).
                let categories: BTreeSet<TransformOp> = chain.operations.iter().copied().collect();
                if categories.len() < 2 {
                    continue;
                }
                // A nested chain is reported by its outermost span only.
                if qualifying.iter().any(|other| {
                    !std::ptr::eq(*other, *chain)
                        && other.span.start_byte <= chain.span.start_byte
                        && other.span.end_byte >= chain.span.end_byte
                        && (other.span.start_byte != chain.span.start_byte
                            || other.span.end_byte != chain.span.end_byte)
                }) {
                    continue;
                }
                let mut rows = vec![evidence(
                    "chain",
                    "patch stack",
                    chain.span.clone(),
                    "three or more transforms applied to one value",
                )];
                for op in &chain.operations {
                    rows.push(evidence(
                        op_label(*op),
                        op_label(*op),
                        chain.span.clone(),
                        "stacked transform operation",
                    ));
                }
                findings.push(finding(
                    SlopKind::PatchStack,
                    rows,
                    "One value is normalized, cast, and defaulted in a stacked chain.",
                    "Validate or convert the value once at its source instead of stacking fixes.",
                ));
            }
        }
        sorted(findings)
    }
}

// ---------------------------------------------------------------------------
// 5. SpeculativeModel
// ---------------------------------------------------------------------------

/// A model whose fields are mostly optional/nullable and mostly unused outside the
/// declaring file — shape built ahead of any real consumer.
struct SpeculativeModel {
    include_test_files: bool,
}

fn field_used_elsewhere(project: &ProjectFacts, model_path: &str, field: &str) -> bool {
    project
        .member_uses
        .get(field)
        .is_some_and(|spans| spans.iter().any(|span| span.path != model_path))
}

impl Detector for SpeculativeModel {
    fn kind(&self) -> SlopKind {
        SlopKind::SpeculativeModel
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            for model in &file.models {
                let exported_declaration = file.declarations.iter().any(|declaration| {
                    declaration.key.name == model.key.name && !declaration.exported_as.is_empty()
                });
                if model.exported
                    || exported_declaration
                    || file.export_surface.names.contains_key(&model.key.name)
                    || [
                        "Props",
                        "Dto",
                        "Request",
                        "Response",
                        "Params",
                        "Settings",
                        "Update",
                        "Preferences",
                        "Args",
                        "Result",
                    ]
                    .iter()
                    .any(|suffix| model.key.name.ends_with(suffix))
                {
                    continue;
                }
                let total = model.fields.len();
                if total < 4 {
                    continue;
                }
                // Signal A: >= 60% of fields are optional or nullable.
                let loose =
                    model.fields.iter().filter(|field| field.optional || field.nullable).count();
                if loose * 100 < total * 60 {
                    continue;
                }
                // Signal B: >= half of fields never accessed outside the declaring file.
                let unused: Vec<&_> = model
                    .fields
                    .iter()
                    .filter(|field| !field_used_elsewhere(project, &model.key.path, &field.name))
                    .collect();
                if unused.len() * 2 < total {
                    continue;
                }
                let mut rows = vec![evidence(
                    &model.key.name,
                    "speculative model",
                    model.span.clone(),
                    "mostly-optional shape with unused fields",
                )];
                for field in unused.iter().take(5) {
                    rows.push(evidence(
                        &field.name,
                        "unused field",
                        field.span.clone(),
                        "field is never read outside this file",
                    ));
                }
                findings.push(finding(
                    SlopKind::SpeculativeModel,
                    rows,
                    "This model is mostly optional fields that nothing outside the file reads.",
                    "Model only the fields that are actually consumed and add the rest when needed.",
                ));
            }
        }
        sorted(findings)
    }
}

// ---------------------------------------------------------------------------
// 6. CommentInversion
// ---------------------------------------------------------------------------

/// Trivial statements carry narrating comments while the file's most complex function
/// carries none.
struct CommentInversion {
    include_test_files: bool,
}

impl Detector for CommentInversion {
    fn kind(&self) -> SlopKind {
        SlopKind::CommentInversion
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            // Signal A: at least three comments that merely restate the next statement.
            let narrating: Vec<&_> =
                file.comments.iter().filter(|comment| comment.narrates_trivial).collect();
            if narrating.len() < 3 {
                continue;
            }
            // Signal B: the highest-complexity function (>= 6) has no comment inside it.
            let complex =
                file.declarations.iter().filter(|decl| is_function_kind(decl.kind)).max_by(
                    |left, right| {
                        left.branch_complexity
                            .cmp(&right.branch_complexity)
                            .then(left.key.cmp(&right.key))
                    },
                );
            let Some(complex) = complex else { continue };
            if complex.branch_complexity < 6 {
                continue;
            }
            let has_inner_comment = file.comments.iter().any(|comment| {
                comment.span.start_byte >= complex.span.start_byte
                    && comment.span.end_byte <= complex.span.end_byte
            });
            if has_inner_comment {
                continue;
            }
            let mut rows = vec![evidence(
                &complex.key.name,
                "uncommented complex function",
                complex.span.clone(),
                "the file's most complex function has no explanatory comment",
            )];
            for comment in narrating.iter().take(3) {
                rows.push(evidence(
                    "comment",
                    "narrating comment",
                    comment.span.clone(),
                    "comment restates the statement it sits on",
                ));
            }
            findings.push(finding(
                SlopKind::CommentInversion,
                rows,
                "Trivial lines are narrated while the file's most complex function is unexplained.",
                "Drop the restating comments and document the complex function's intent instead.",
            ));
        }
        sorted(findings)
    }
}

// ---------------------------------------------------------------------------
// 7. ParallelRepresentation
// ---------------------------------------------------------------------------

/// Two or more models that share >= 85% of their field names but disagree on
/// optionality or primitive type — near-duplicate shapes that drifted apart.
struct ParallelRepresentation {
    include_test_files: bool,
}

fn normalized_names(model: &ModelFact) -> BTreeSet<String> {
    model.fields.iter().map(|field| field.name.to_ascii_lowercase().replace('_', "")).collect()
}

/// Resolved primitive type of a model field, from the type-resolved `model_field_types`
/// index (the raw `FieldFact.primitive` is unreliable). `None` when unresolved.
fn resolved_primitive(project: &ProjectFacts, model: &ModelFact, field: &str) -> Option<String> {
    match &project.model_field_types.get(&(model.key.clone(), field.to_string()))?.primitive {
        Resolution::Resolved(primitive) => Some(primitive.clone()),
        _ => None,
    }
}

/// Per-field shape used to decide "not byte-identical": normalized name plus
/// optionality, nullability, and resolved primitive type.
fn model_signature(
    project: &ProjectFacts,
    model: &ModelFact,
) -> BTreeSet<(String, bool, bool, Option<String>)> {
    model
        .fields
        .iter()
        .map(|field| {
            (
                field.name.to_ascii_lowercase().replace('_', ""),
                field.optional,
                field.nullable,
                resolved_primitive(project, model, &field.name),
            )
        })
        .collect()
}

fn parallel_pair(project: &ProjectFacts, left: &ModelFact, right: &ModelFact) -> bool {
    let left_names = normalized_names(left);
    let right_names = normalized_names(right);
    if left_names.is_empty() || right_names.is_empty() {
        return false;
    }
    // Signal A: >= 85% Jaccard overlap of field names, and not identical shapes.
    let intersection = left_names.intersection(&right_names).count();
    let union = left_names.union(&right_names).count();
    if intersection * 100 < union * 85 {
        return false;
    }
    if model_signature(project, left) == model_signature(project, right) {
        return false;
    }
    // Signal B: at least one shared field differs in optionality or resolved primitive.
    left.fields.iter().any(|field| {
        let key = field.name.to_ascii_lowercase().replace('_', "");
        let left_primitive = resolved_primitive(project, left, &field.name);
        right.fields.iter().any(|other| {
            if other.name.to_ascii_lowercase().replace('_', "") != key {
                return false;
            }
            if other.optional != field.optional {
                return true;
            }
            let right_primitive = resolved_primitive(project, right, &other.name);
            left_primitive.is_some()
                && right_primitive.is_some()
                && left_primitive != right_primitive
        })
    })
}

impl Detector for ParallelRepresentation {
    fn kind(&self) -> SlopKind {
        SlopKind::ParallelRepresentation
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        // Collect in-scope models with >= 1 field, keyed and ordered by TypeKey.
        let models: BTreeMap<TypeKey, &ModelFact> = project
            .files
            .values()
            .filter(|file| !skipped(file, self.include_test_files))
            .flat_map(|file| file.models.iter())
            .filter(|model| !model.fields.is_empty())
            .map(|model| (model.key.clone(), model))
            .collect();
        let keys: Vec<&TypeKey> = models.keys().collect();

        // Build an undirected adjacency between parallel models.
        let mut adjacency: BTreeMap<&TypeKey, BTreeSet<&TypeKey>> = BTreeMap::new();
        for (index, left) in keys.iter().enumerate() {
            for right in &keys[index + 1..] {
                if parallel_pair(project, models[*left], models[*right]) {
                    adjacency.entry(left).or_default().insert(right);
                    adjacency.entry(right).or_default().insert(left);
                }
            }
        }

        // Emit one finding per connected component, anchored at its smallest key.
        let mut findings = Vec::new();
        let mut visited: BTreeSet<&TypeKey> = BTreeSet::new();
        for start in &keys {
            if !adjacency.contains_key(start) || visited.contains(*start) {
                continue;
            }
            let mut component = BTreeSet::new();
            let mut stack = vec![*start];
            while let Some(node) = stack.pop() {
                if !visited.insert(node) {
                    continue;
                }
                component.insert(node);
                if let Some(neighbors) = adjacency.get(node) {
                    stack.extend(neighbors.iter().copied());
                }
            }
            let rows: Vec<SlopEvidence> = component
                .iter()
                .map(|key| {
                    evidence(
                        &key.name,
                        "parallel shape",
                        models[*key].span.clone(),
                        "near-duplicate model that differs in optionality or type",
                    )
                })
                .collect();
            findings.push(finding(
                SlopKind::ParallelRepresentation,
                rows,
                "These models describe the same shape but disagree on optionality or field types.",
                "Unify them into one model and derive any variants from it.",
            ));
        }
        sorted(findings)
    }
}

// ---------------------------------------------------------------------------
// 8. GenericNameCluster
// ---------------------------------------------------------------------------

/// Denylist of low-information names (exact match, case-insensitive).
const GENERIC_NAMES: &[&str] = &[
    "data", "info", "item", "items", "obj", "temp", "tmp", "result", "res", "value", "val",
    "handler", "process", "manager", "util", "utils", "helper", "helpers", "do", "run", "thing",
    "foo", "bar", "baz", "ret", "cb", "fn", "func", "arg", "args", "param", "params", "options",
    "config", "settings", "wrapper", "x", "y", "z", "e", "ev", "el", "node",
];

/// A file with four or more generic declaration names, at least two of which carry
/// real weight (high complexity or high coupling).
struct GenericNameCluster {
    include_test_files: bool,
}

fn is_generic_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    GENERIC_NAMES.contains(&lower.as_str())
}

impl Detector for GenericNameCluster {
    fn kind(&self) -> SlopKind {
        SlopKind::GenericNameCluster
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for file in project.files.values() {
            if skipped(file, self.include_test_files) {
                continue;
            }
            let generic: Vec<&DeclarationFact> =
                file.declarations.iter().filter(|decl| is_generic_name(&decl.key.name)).collect();
            // Signal A: at least four generic-named declarations.
            if generic.len() < 4 {
                continue;
            }
            // Signal B: at least two are high-complexity or high-coupling functions.
            let weighty = generic
                .iter()
                .filter(|decl| {
                    is_function_kind(decl.kind)
                        && (decl.branch_complexity >= 6 || uses_of(project, &decl.key).len() >= 4)
                })
                .count();
            if weighty < 2 {
                continue;
            }
            let rows: Vec<SlopEvidence> = generic
                .iter()
                .take(6)
                .map(|decl| {
                    evidence(
                        &decl.key.name,
                        "generic name",
                        decl.span.clone(),
                        "low-information declaration name",
                    )
                })
                .collect();
            findings.push(finding(
                SlopKind::GenericNameCluster,
                rows,
                "This file clusters generic names, some on load-bearing functions.",
                "Rename the load-bearing declarations to describe what they hold or do.",
            ));
        }
        sorted(findings)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;
    use crate::slop::{build_project_facts, collect_project_files};

    fn facts_from(files: &[(&str, &str)]) -> ProjectFacts {
        let root = tempdir().unwrap();
        for (name, source) in files {
            let path = root.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, source).unwrap();
        }
        let canonical = root.path().canonicalize().unwrap();
        build_project_facts(&canonical, collect_project_files(&canonical).unwrap()).unwrap()
    }

    fn run(detector: &dyn Detector, project: &ProjectFacts) -> Vec<SlopFinding> {
        detector.detect(project)
    }

    fn options() -> SlopOptions {
        SlopOptions::default()
    }

    fn only(detectors: Vec<Box<dyn Detector>>, kind: SlopKind) -> Box<dyn Detector> {
        detectors.into_iter().find(|detector| detector.kind() == kind).unwrap()
    }

    #[test]
    fn registers_all_eight_owned_detectors_at_medium() {
        let registry = detectors(&options());
        let kinds: Vec<SlopKind> = registry.iter().map(|detector| detector.kind()).collect();
        assert_eq!(
            kinds,
            vec![
                SlopKind::ReinventedHelper,
                SlopKind::RedundantDefense,
                SlopKind::OneUseAbstraction,
                SlopKind::PatchStack,
                SlopKind::SpeculativeModel,
                SlopKind::CommentInversion,
                SlopKind::ParallelRepresentation,
                SlopKind::GenericNameCluster,
            ]
        );
        assert!(registry.iter().all(|detector| detector.confidence() == SlopConfidence::Medium));
    }

    // --- 1. ReinventedHelper -------------------------------------------------

    // Three-line bodies so the similarity engine (min_lines = 3) forms a group.
    const SHARED_HELPER: &str = "\
export function formatMoney(cents: number): string {
  const dollars = cents / 100;
  const text = dollars.toFixed(2);
  return `$${text}`;
}
";
    const REIMPLEMENTED_HELPER: &str = "\
export function formatMoney(cents: number): string {
  const dollars = cents / 100;
  const text = dollars.toFixed(2);
  return `$${text}`;
}
";
    const HELPER_CONSUMER: &str = "\
import { formatMoney } from './money';
export function price(cents: number): string {
  const out = formatMoney(cents);
  return out;
}
";

    #[test]
    fn reinvented_helper_flags_local_reimplementation() {
        let project = facts_from(&[
            ("money.ts", SHARED_HELPER),
            ("consumer.ts", HELPER_CONSUMER),
            ("report.ts", REIMPLEMENTED_HELPER),
        ]);
        let detector = only(detectors(&options()), SlopKind::ReinventedHelper);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        let finding = &findings[0];
        assert_eq!(finding.confidence, SlopConfidence::Medium);
        assert_eq!(finding.evidence.len(), 2);
        assert_eq!(finding.evidence[0].span.path, "report.ts");
        assert!(finding.evidence.iter().any(|row| row.span.path == "money.ts"));
    }

    #[test]
    fn reinvented_helper_ignores_files_that_import_the_helper() {
        // report.ts imports the real helper, so its structural twin is not reinvention.
        let importer = "\
import { formatMoney } from './money';
export function formatCash(cents: number): string {
  const dollars = cents / 100;
  const text = dollars.toFixed(2);
  return `$${text}`;
}
export function ticket(cents: number): string {
  return formatMoney(cents);
}
";
        let project = facts_from(&[
            ("money.ts", SHARED_HELPER),
            ("consumer.ts", HELPER_CONSUMER),
            ("report.ts", importer),
        ]);
        let detector = only(detectors(&options()), SlopKind::ReinventedHelper);
        assert!(run(&*detector, &project).is_empty());
    }

    #[test]
    fn reinvented_helper_picks_smallest_established_exemplar() {
        let project = facts_from(&[
            ("money.ts", SHARED_HELPER),
            ("consumer.ts", HELPER_CONSUMER),
            ("alt.ts", REIMPLEMENTED_HELPER),
            (
                "alt_consumer.ts",
                "import { formatMoney } from './alt';\nexport function altPrice(c: number): string {\n  const out = formatMoney(c);\n  return out;\n}\n",
            ),
            ("report.ts", REIMPLEMENTED_HELPER),
        ]);
        let detector = only(detectors(&options()), SlopKind::ReinventedHelper);
        let findings = run(&*detector, &project);
        // report.ts reimplements; exemplar is the lexicographically smallest established file.
        assert!(!findings.is_empty());
        let report_finding = findings.iter().find(|f| f.span.path == "report.ts").unwrap();
        let exemplar = &report_finding.evidence[1];
        assert_eq!(exemplar.span.path, "alt.ts");
    }

    // --- 2. RedundantDefense ------------------------------------------------

    #[test]
    fn redundant_defense_flags_guard_after_validator_proof() {
        let project = facts_from(&[(
            "guard.ts",
            "declare const schema: { parse(v: unknown): { id: string } };\nfunction handle(input: unknown): string {\n  const user = schema.parse(input);\n  if (user == null) { return 'x'; }\n  return user.id;\n}\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::RedundantDefense);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].confidence, SlopConfidence::Medium);
        assert_eq!(findings[0].evidence.len(), 2);
        assert!(findings[0].evidence.iter().any(|row| row.label == "non-null proof"));
    }

    #[test]
    fn redundant_defense_ignores_genuinely_nullable_value() {
        let project = facts_from(&[(
            "guard.ts",
            "function handle(user: { id: string } | null): string {\n  if (user == null) { return 'x'; }\n  return user.id;\n}\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::RedundantDefense);
        assert!(run(&*detector, &project).is_empty());
    }

    #[test]
    fn redundant_defense_ignores_exported_parameter_boundary() {
        // A guard on a required non-null parameter of an exported function is
        // legitimate input validation, not a redundant guard on a proven value.
        let project = facts_from(&[(
            "guard.ts",
            "export function handle(user: { id: string }): string {\n  if (user == null) { return 'x'; }\n  return user.id;\n}\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::RedundantDefense);
        assert!(run(&*detector, &project).is_empty());
    }

    #[test]
    fn redundant_defense_flags_non_null_assertion_on_proven_value() {
        let project = facts_from(&[(
            "guard.ts",
            "function handle(): string {\n  const user = { id: 'a' };\n  const value = user!;\n  return value.id;\n}\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::RedundantDefense);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert!(
            findings[0].evidence.iter().any(|row| row.label == "redundant non-null assertion"),
            "{findings:#?}"
        );
    }

    #[test]
    fn redundant_defense_ignores_member_guards_and_call_results() {
        let project = facts_from(&[(
            "guard.ts",
            "function handle(value: { child: string | null }): string {\n  if (value.child !== null) return value.child;\n  const map = new Map<string, string>();\n  return map.get('fallback')!;\n}\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::RedundantDefense);
        assert!(run(&*detector, &project).is_empty());
    }

    // --- 3. OneUseAbstraction -----------------------------------------------

    #[test]
    fn one_use_abstraction_flags_single_caller_wrapper() {
        let project = facts_from(&[
            ("inner.ts", "export function inner(value: number): number { return value + 1; }\n"),
            (
                "wrap.ts",
                "import { inner } from './inner';\nfunction wrap(value: number): number { return inner(value); }\nexport function caller(value: number): number { return wrap(value); }\n",
            ),
        ]);
        let detector = only(detectors(&options()), SlopKind::OneUseAbstraction);
        let findings = run(&*detector, &project);
        let wrapper = findings.iter().find(|f| f.evidence.iter().any(|row| row.code == "wrap"));
        assert!(wrapper.is_some(), "{findings:#?}");
        assert_eq!(wrapper.unwrap().evidence.len(), 2);
    }

    #[test]
    fn one_use_abstraction_ignores_wrappers_with_real_logic() {
        let project = facts_from(&[
            ("inner.ts", "export function inner(value: number): number { return value + 1; }\n"),
            (
                "wrap.ts",
                "import { inner } from './inner';\nfunction wrap(value: number): number { const doubled = value * 2; return inner(doubled); }\nexport function caller(value: number): number { return wrap(value); }\n",
            ),
        ]);
        let detector = only(detectors(&options()), SlopKind::OneUseAbstraction);
        let findings = run(&*detector, &project);
        assert!(findings.iter().all(|f| !f.evidence.iter().any(|row| row.code == "wrap")));
    }

    #[test]
    fn one_use_abstraction_flags_single_implementer_interface() {
        let project = facts_from(&[
            ("shape.ts", "export interface Shape { id: string }\n"),
            (
                "box.ts",
                "import type { Shape } from './shape';\nexport interface Box extends Shape { size: number }\n",
            ),
        ]);
        let detector = only(detectors(&options()), SlopKind::OneUseAbstraction);
        let findings = run(&*detector, &project);
        assert!(
            findings.iter().any(|f| f.evidence.iter().any(|row| row.code == "Shape")),
            "{findings:#?}"
        );
    }

    // --- 4. PatchStack -------------------------------------------------------

    #[test]
    fn patch_stack_flags_three_category_chain() {
        let project = facts_from(&[(
            "chain.ts",
            "declare function normalizeInput(v: unknown): unknown;\ndeclare function castValue(v: unknown): unknown;\ndeclare function withDefault(v: unknown): unknown;\nexport function build(raw: unknown): unknown { return normalizeInput(castValue(withDefault(raw))); }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::PatchStack);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert!(findings[0].evidence.len() >= 3);
    }

    #[test]
    fn patch_stack_ignores_single_category_cast_stack() {
        let project = facts_from(&[(
            "chain.ts",
            "declare function castA(v: unknown): unknown;\ndeclare function castB(v: unknown): unknown;\ndeclare function castC(v: unknown): unknown;\nexport function build(raw: unknown): unknown { return castC(castB(castA(raw))); }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::PatchStack);
        assert!(run(&*detector, &project).is_empty());
    }

    #[test]
    fn patch_stack_edge_two_categories_emits_two_ops_does_not() {
        // Three ops across two categories emits.
        let two_categories = facts_from(&[(
            "chain.ts",
            "declare function castA(v: unknown): unknown;\ndeclare function castB(v: unknown): unknown;\ndeclare function withDefault(v: unknown): unknown;\nexport function build(raw: unknown): unknown { return castA(castB(withDefault(raw))); }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::PatchStack);
        assert_eq!(run(&*detector, &two_categories).len(), 1);

        // Two ops never reaches Signal A.
        let two_ops = facts_from(&[(
            "chain.ts",
            "declare function castA(v: unknown): unknown;\ndeclare function withDefault(v: unknown): unknown;\nexport function build(raw: unknown): unknown { return castA(withDefault(raw)); }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::PatchStack);
        assert!(run(&*detector, &two_ops).is_empty());
    }

    // --- 5. SpeculativeModel -------------------------------------------------

    #[test]
    fn speculative_model_flags_mostly_optional_unused_shape() {
        let project = facts_from(&[(
            "dto.ts",
            "interface InternalShape { a?: string; b?: string; c?: string; d?: string; e?: string; f: string }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::SpeculativeModel);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert!(findings[0].evidence.len() >= 2);
    }

    #[test]
    fn speculative_model_ignores_fields_read_elsewhere() {
        let project = facts_from(&[
            (
                "dto.ts",
                "export interface Dto { a?: string; b?: string; c?: string; d?: string; e?: string; f: string }\n",
            ),
            (
                "reader.ts",
                "import type { Dto } from './dto';\nexport function read(dto: Dto): string { return `${dto.a}${dto.b}${dto.c}${dto.d}${dto.e}${dto.f}`; }\n",
            ),
        ]);
        let detector = only(detectors(&options()), SlopKind::SpeculativeModel);
        assert!(run(&*detector, &project).is_empty());
    }

    #[test]
    fn speculative_model_ignores_public_and_props_boundaries() {
        let project = facts_from(&[(
            "dto.tsx",
            "export interface PublicDto { a?: string; b?: string; c?: string; d?: string }\ninterface LocalProps { a?: string; b?: string; c?: string; d?: string }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::SpeculativeModel);
        let findings = run(&*detector, &project);
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn speculative_model_requires_four_fields() {
        let project = facts_from(&[(
            "dto.ts",
            "export interface Dto { a?: string; b?: string; c: string }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::SpeculativeModel);
        assert!(run(&*detector, &project).is_empty());
    }

    // --- 6. CommentInversion -------------------------------------------------

    // Each comment's words are a subset of the statement below it (the foundation's
    // `narrates_trivial` heuristic). `classify` reaches complexity >= 6 via plain `if`
    // statements, which the corrected `branch_complexity` now counts.
    const NARRATED_COMPLEX: &str = "\
const count = 1;
// total count
const total = count + count;
// label total
const label = total > total;
// value label
const value = label;
export function classify(n: number): number {
  if (n > 0) { return 1; }
  if (n > 1) { return 2; }
  if (n > 2) { return 3; }
  if (n > 3) { return 4; }
  if (n > 4) { return 5; }
  if (n > 5) { return 6; }
  return 0;
}
";

    #[test]
    fn comment_inversion_flags_narration_with_uncommented_complexity() {
        let project = facts_from(&[("mod.ts", NARRATED_COMPLEX)]);
        let detector = only(detectors(&options()), SlopKind::CommentInversion);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert!(findings[0].evidence.iter().any(|row| row.label == "narrating comment"));
    }

    #[test]
    fn comment_inversion_ignores_documented_complex_function() {
        let documented = "\
const count = 1;
// total count
const total = count + count;
// label total
const label = total > total;
// value label
const value = label;
export function classify(n: number): number {
  // intent: bucket the number by magnitude
  if (n > 0) { return 1; }
  if (n > 1) { return 2; }
  if (n > 2) { return 3; }
  if (n > 3) { return 4; }
  if (n > 4) { return 5; }
  if (n > 5) { return 6; }
  return 0;
}
";
        let project = facts_from(&[("mod.ts", documented)]);
        let detector = only(detectors(&options()), SlopKind::CommentInversion);
        assert!(run(&*detector, &project).is_empty());
    }

    // --- 7. ParallelRepresentation ------------------------------------------

    #[test]
    fn parallel_representation_flags_optionality_drift() {
        let project = facts_from(&[(
            "models.ts",
            "export interface UserA { id: string; name?: string }\nexport interface UserB { id: string; name: string }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::ParallelRepresentation);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].evidence.len(), 2);
    }

    #[test]
    fn parallel_representation_ignores_identical_aliases() {
        let project = facts_from(&[(
            "models.ts",
            "export interface UserA { id: string; name: string }\nexport interface UserB { id: string; name: string }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::ParallelRepresentation);
        assert!(run(&*detector, &project).is_empty());
    }

    #[test]
    fn parallel_representation_flags_primitive_drift() {
        // Resolved primitives from `model_field_types` distinguish `age: number` from
        // `age: string`, so a difference that is only a primitive-type change emits.
        let project = facts_from(&[(
            "models.ts",
            "export interface UserA { id: string; age: number }\nexport interface UserB { id: string; age: string }\n",
        )]);
        let detector = only(detectors(&options()), SlopKind::ParallelRepresentation);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].evidence.len(), 2);
    }

    // --- 8. GenericNameCluster ----------------------------------------------

    // `process`/`run` reach complexity >= 6 via plain `if` statements (now counted);
    // `data`, `info`, `result`, `process`, `run` are all denylisted generic names.
    const GENERIC_CLUSTER: &str = "\
export const data = 1;
export const info = 2;
export const result = 3;
export function process(n: number): number {
  if (n > 0) { return 1; }
  if (n > 1) { return 2; }
  if (n > 2) { return 3; }
  if (n > 3) { return 4; }
  if (n > 4) { return 5; }
  if (n > 5) { return 6; }
  return 0;
}
export function run(n: number): number {
  if (n > 0) { return 1; }
  if (n > 1) { return 2; }
  if (n > 2) { return 3; }
  if (n > 3) { return 4; }
  if (n > 4) { return 5; }
  if (n > 5) { return 6; }
  return 0;
}
";

    #[test]
    fn generic_name_cluster_flags_weighty_cluster() {
        let project = facts_from(&[("mod.ts", GENERIC_CLUSTER)]);
        let detector = only(detectors(&options()), SlopKind::GenericNameCluster);
        let findings = run(&*detector, &project);
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert!(findings[0].evidence.len() >= 4);
    }

    #[test]
    fn generic_name_cluster_ignores_simple_names() {
        let simple = "\
export const data = 1;
export const info = 2;
export const result = 3;
export const value = 4;
";
        let project = facts_from(&[("mod.ts", simple)]);
        let detector = only(detectors(&options()), SlopKind::GenericNameCluster);
        assert!(run(&*detector, &project).is_empty());
    }

    // --- Cross-cutting: test-file exclusion ---------------------------------

    #[test]
    fn detectors_skip_test_files_by_default_but_honor_opt_in() {
        let default_project = facts_from(&[(
            "dto.test.ts",
            "interface InternalShape { a?: string; b?: string; c?: string; d?: string; e?: string; f: string }\n",
        )]);
        let default_detector = only(detectors(&options()), SlopKind::SpeculativeModel);
        assert!(run(&*default_detector, &default_project).is_empty());

        let opts = SlopOptions { include_test_files: true };
        let opt_in_detector = only(detectors(&opts), SlopKind::SpeculativeModel);
        assert_eq!(run(&*opt_in_detector, &default_project).len(), 1);
    }

    #[test]
    fn findings_are_deterministic_and_medium() {
        let project = facts_from(&[("mod.ts", GENERIC_CLUSTER)]);
        for detector in detectors(&options()) {
            let first = detector.detect(&project);
            let second = detector.detect(&project);
            assert_eq!(first, second);
            assert!(first.iter().all(|f| f.confidence == SlopConfidence::Medium));
        }
    }

    // Keep `Path` import meaningful for readers cross-referencing fixture roots.
    const _: fn(&Path) = |_| {};
}
