#![allow(dead_code)] // Stable Phase 1–3 contracts are consumed by later detector lanes.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::commands::dupes::FunctionDuplicateKey;
use crate::scan::types::FunctionRole;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum SlopConfidence {
    High,
    #[default]
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SlopKind {
    SuppressionChain,
    SwallowedFailure,
    UnresolvedApi,
    AsyncMisuse,
    DeadSurface,
    NonExecutingTest,
    ReinventedHelper,
    LowValueLocalHelper,
    DominantContainerTinyHelpers,
    RedundantDefense,
    OneUseAbstraction,
    PatchStack,
    SpeculativeModel,
    CommentInversion,
    ParallelRepresentation,
    GenericNameCluster,
    AssertionMonoculture,
    MockDominatedTest,
    DuplicatedTestBody,
    ImplementationMirroringTest,
    ScopeInflation,
    IntroducedReinvention,
    GeneratedSurfaceBurst,
}

impl SlopKind {
    pub const ALL: [Self; 23] = [
        Self::SuppressionChain,
        Self::SwallowedFailure,
        Self::UnresolvedApi,
        Self::AsyncMisuse,
        Self::DeadSurface,
        Self::NonExecutingTest,
        Self::ReinventedHelper,
        Self::LowValueLocalHelper,
        Self::DominantContainerTinyHelpers,
        Self::RedundantDefense,
        Self::OneUseAbstraction,
        Self::PatchStack,
        Self::SpeculativeModel,
        Self::CommentInversion,
        Self::ParallelRepresentation,
        Self::GenericNameCluster,
        Self::AssertionMonoculture,
        Self::MockDominatedTest,
        Self::DuplicatedTestBody,
        Self::ImplementationMirroringTest,
        Self::ScopeInflation,
        Self::IntroducedReinvention,
        Self::GeneratedSurfaceBurst,
    ];

    pub const fn cli_name(self) -> &'static str {
        match self {
            Self::SuppressionChain => "suppression-chain",
            Self::SwallowedFailure => "swallowed-failure",
            Self::UnresolvedApi => "unresolved-api",
            Self::AsyncMisuse => "async-misuse",
            Self::DeadSurface => "dead-surface",
            Self::NonExecutingTest => "non-executing-test",
            Self::ReinventedHelper => "reinvented-helper",
            Self::LowValueLocalHelper => "low-value-local-helper",
            Self::DominantContainerTinyHelpers => "dominant-container-tiny-helpers",
            Self::RedundantDefense => "redundant-defense",
            Self::OneUseAbstraction => "one-use-abstraction",
            Self::PatchStack => "patch-stack",
            Self::SpeculativeModel => "speculative-model",
            Self::CommentInversion => "comment-inversion",
            Self::ParallelRepresentation => "parallel-representation",
            Self::GenericNameCluster => "generic-name-cluster",
            Self::AssertionMonoculture => "assertion-monoculture",
            Self::MockDominatedTest => "mock-dominated-test",
            Self::DuplicatedTestBody => "duplicated-test-body",
            Self::ImplementationMirroringTest => "implementation-mirroring-test",
            Self::ScopeInflation => "scope-inflation",
            Self::IntroducedReinvention => "introduced-reinvention",
            Self::GeneratedSurfaceBurst => "generated-surface-burst",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::SuppressionChain => "Suppression chain",
            Self::SwallowedFailure => "Swallowed failure",
            Self::UnresolvedApi => "Unresolved API",
            Self::AsyncMisuse => "Async misuse",
            Self::DeadSurface => "Dead surface",
            Self::NonExecutingTest => "Non-executing test",
            Self::ReinventedHelper => "Reinvented helper",
            Self::LowValueLocalHelper => "Low-value local helper",
            Self::DominantContainerTinyHelpers => "Dominant container with tiny helpers",
            Self::RedundantDefense => "Redundant defense",
            Self::OneUseAbstraction => "One-use abstraction",
            Self::PatchStack => "Patch stack",
            Self::SpeculativeModel => "Speculative model",
            Self::CommentInversion => "Comment inversion",
            Self::ParallelRepresentation => "Parallel representation",
            Self::GenericNameCluster => "Generic-name cluster",
            Self::AssertionMonoculture => "Assertion monoculture",
            Self::MockDominatedTest => "Mock-dominated test",
            Self::DuplicatedTestBody => "Duplicated test body",
            Self::ImplementationMirroringTest => "Implementation-mirroring test",
            Self::ScopeInflation => "Scope inflation",
            Self::IntroducedReinvention => "Introduced reinvention",
            Self::GeneratedSurfaceBurst => "Generated surface burst",
        }
    }

    pub const fn sort_rank(self) -> u8 {
        match self {
            Self::SuppressionChain => 0,
            Self::SwallowedFailure => 1,
            Self::UnresolvedApi => 2,
            Self::AsyncMisuse => 3,
            Self::DeadSurface => 4,
            Self::NonExecutingTest => 5,
            Self::ReinventedHelper => 6,
            Self::LowValueLocalHelper => 7,
            Self::DominantContainerTinyHelpers => 8,
            Self::RedundantDefense => 9,
            Self::OneUseAbstraction => 10,
            Self::PatchStack => 11,
            Self::SpeculativeModel => 12,
            Self::CommentInversion => 13,
            Self::ParallelRepresentation => 14,
            Self::GenericNameCluster => 15,
            Self::AssertionMonoculture => 16,
            Self::MockDominatedTest => 17,
            Self::DuplicatedTestBody => 18,
            Self::ImplementationMirroringTest => 19,
            Self::ScopeInflation => 20,
            Self::IntroducedReinvention => 21,
            Self::GeneratedSurfaceBurst => 22,
        }
    }

    pub const fn is_diff_only(self) -> bool {
        matches!(
            self,
            Self::ScopeInflation | Self::IntroducedReinvention | Self::GeneratedSurfaceBurst
        )
    }

    pub fn from_cli_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.cli_name() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSpan {
    pub path: String,
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolKey {
    pub path: String,
    pub declaration_start: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallSiteKey {
    pub path: String,
    pub start_byte: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ScopeKey {
    Module(String),
    Function(SymbolKey),
    Test { path: String, call_start: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status", content = "value")]
pub enum Resolution<T> {
    Resolved(T),
    Missing { attempted: Vec<String> },
    Ambiguous { candidates: Vec<T> },
    Unknown { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SuppressionDirectiveKind {
    TsIgnore,
    EslintDisable,
    EslintDisableNextLine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentFact {
    pub span: SourceSpan,
    pub scope: ScopeKey,
    pub directive: Option<SuppressionDirectiveKind>,
    pub lint_rules: Vec<String>,
    pub target: Option<SourceSpan>,
    pub placeholder: bool,
    pub narrates_trivial: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CastKind {
    AsAny,
    TypeAssertionAny,
    OtherAs,
    OtherTypeAssertion,
    NonNull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CastFact {
    pub span: SourceSpan,
    pub operand_span: SourceSpan,
    pub expression_root: SourceSpan,
    pub scope: ScopeKey,
    pub kind: CastKind,
    pub nesting_depth: u16,
    pub nested_assertion_count: u16,
    pub target_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CatchEffectKind {
    Log,
    Telemetry,
    Throw,
    Reject,
    Return,
    OtherCall,
    Mutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReturnShape {
    None,
    Undefined,
    Null,
    False,
    True,
    EmptyString,
    EmptyArray,
    EmptyObject,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatchFact {
    pub span: SourceSpan,
    pub body_span: SourceSpan,
    pub scope: ScopeKey,
    pub parameter_name: Option<String>,
    pub top_level_statement_count: usize,
    pub effects: Vec<(CatchEffectKind, SourceSpan)>,
    pub return_shape: ReturnShape,
    pub can_fall_through: bool,
    pub has_nested_function: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromiseCatchFact {
    pub call_span: SourceSpan,
    pub callback_span: Option<SourceSpan>,
    pub scope: ScopeKey,
    pub callback: Resolution<CatchFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConstantCondition {
    AlwaysTrue,
    AlwaysFalse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchFact {
    pub span: SourceSpan,
    pub condition_span: SourceSpan,
    pub scope: ScopeKey,
    pub condition: Option<ConstantCondition>,
    pub unreachable_span: Option<SourceSpan>,
    pub adjacent_placeholder_comment: Option<SourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeclarationKind {
    Function,
    Method,
    Variable,
    Class,
    Interface,
    TypeAlias,
    Enum,
    Property,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BodyShape {
    Empty,
    ConstantReturn,
    IdentityReturn,
    PropertyReturn,
    PassThroughCall,
    Other,
}

#[allow(clippy::struct_excessive_bools)] // Independent neutral syntax properties, not state flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationFact {
    pub key: SymbolKey,
    pub span: SourceSpan,
    pub body_span: Option<SourceSpan>,
    pub scope: ScopeKey,
    pub kind: DeclarationKind,
    pub exported_as: Vec<String>,
    pub ambient: bool,
    pub has_body: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub role: Option<FunctionRole>,
    pub body_shape: BodyShape,
    pub parameter_count: Option<usize>,
    pub branch_complexity: usize,
    pub control_nesting: usize,
    pub await_spans: Vec<SourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallResultUse {
    Awaited,
    Returned,
    Yielded,
    Voided,
    PromiseHandler,
    PromiseCombinatorArgument,
    Assigned,
    Argument,
    Condition,
    FloatingExpressionStatement,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AsyncFact {
    pub key: CallSiteKey,
    pub span: SourceSpan,
    pub scope: ScopeKey,
    pub callee_path: Vec<String>,
    pub callee_symbol: Resolution<SymbolKey>,
    pub result_use: CallResultUse,
    pub nearest_loop: Option<SourceSpan>,
    pub await_span: Option<SourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportSpecifierKind {
    SideEffect,
    Default,
    Namespace,
    Named,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFact {
    pub span: SourceSpan,
    pub source: String,
    pub kind: ImportSpecifierKind,
    pub imported: Option<String>,
    pub local: Option<String>,
    pub type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberUseFact {
    pub span: SourceSpan,
    pub scope: ScopeKey,
    pub base_name: Option<String>,
    pub static_member: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GuardKind {
    NullCheck,
    TypeofCheck,
    OptionalChain,
    NonNullAssertion,
    NullishDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardFact {
    pub span: SourceSpan,
    pub scope: ScopeKey,
    pub guarded_symbol: Option<String>,
    pub kind: GuardKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NonNullProofKind {
    RequiredParameter,
    LiteralInitializer,
    NewInitializer,
    ValidatorCall,
    AssertionCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NonNullProof {
    pub kind: NonNullProofKind,
    pub span: SourceSpan,
    pub scope: ScopeKey,
    pub effective_after_byte: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolTypeFact {
    pub key: SymbolKey,
    pub scope: ScopeKey,
    pub primitive: Option<String>,
    pub nullable: bool,
    pub annotation_complete: bool,
    pub proven_nonnull: Resolution<NonNullProof>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransformOp {
    Normalize,
    Cast,
    Default,
    Adapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformChainFact {
    pub span: SourceSpan,
    pub scope: ScopeKey,
    pub root_symbol: Option<String>,
    pub operations: Vec<TransformOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceFact {
    pub name: String,
    pub span: SourceSpan,
    pub resolved: Resolution<SymbolKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallFact {
    pub key: CallSiteKey,
    pub span: SourceSpan,
    pub scope: ScopeKey,
    pub callee_path: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TestMode {
    Run,
    Only,
    Skip,
    Todo,
    DisabledAlias,
    Parameterized,
    Property,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestId {
    pub path: String,
    pub callback_start: u32,
    pub registration_start: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteId {
    pub path: String,
    pub registration_start: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupId {
    pub path: String,
    pub registration_start: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TestFramework {
    JestLike,
    NodeTest,
    Deno,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SetupKind {
    BeforeEach,
    AfterEach,
    BeforeAll,
    AfterAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteFact {
    pub id: SuiteId,
    pub parent: Option<SuiteId>,
    pub name: Option<String>,
    pub span: SourceSpan,
    pub callback_span: Option<SourceSpan>,
    pub resolution_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupFact {
    pub id: SetupId,
    pub suite: SuiteId,
    pub kind: SetupKind,
    pub registration_span: SourceSpan,
    pub callback_span: Option<SourceSpan>,
    pub resolution_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionShape {
    pub canonical: String,
    pub complexity: u16,
    pub call_chain: Vec<String>,
    pub referenced_symbols: Vec<SymbolKey>,
    pub resolution_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestBodyShape {
    pub canonical: String,
    pub literal_vector: Vec<String>,
    pub statement_count: u16,
    pub node_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCaseFact {
    pub id: TestId,
    pub suite: SuiteId,
    pub framework: TestFramework,
    pub registration_span: SourceSpan,
    pub callback_span: SourceSpan,
    pub body_span: SourceSpan,
    pub mode: TestMode,
    pub body_shape: TestBodyShape,
    pub has_snapshot: bool,
    pub has_unknown_dynamic_call: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AssertionApi {
    Expect,
    NodeAssert,
    ChaiAssert,
    UnknownAssert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AssertionBoundary {
    None,
    Error,
    Nullish,
    Empty,
    BooleanEdge,
    NumericEdge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionFact {
    pub span: SourceSpan,
    pub test: Option<TestId>,
    pub api: AssertionApi,
    pub api_resolution: Resolution<String>,
    pub matcher: String,
    pub negated: bool,
    pub async_modifier: Option<String>,
    pub boundary: AssertionBoundary,
    pub actual: Option<ExpressionShape>,
    pub expected: Option<ExpressionShape>,
    pub invoked_call: Option<CallSiteKey>,
    pub invokes: Resolution<SymbolKey>,
    pub is_snapshot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MockKind {
    Factory,
    Module,
    Spy,
    Stub,
    Behavior,
    Restore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MockFact {
    pub span: SourceSpan,
    pub test: Option<TestId>,
    pub suite: SuiteId,
    pub setup: Option<SetupId>,
    pub kind: MockKind,
    pub callee: String,
    pub resolution_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallArgumentFact {
    pub call: CallSiteKey,
    pub arguments: Vec<ExpressionShape>,
    pub has_spread: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallRole {
    Sut,
    Assertion,
    Mock,
    TestFramework,
    Fixture,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionExpressionFact {
    pub owner: SymbolKey,
    pub owner_span: SourceSpan,
    pub expression_span: SourceSpan,
    pub parameter_names: Vec<String>,
    pub returned: ExpressionShape,
    pub eligible: bool,
    pub ambiguity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestFact {
    pub call_span: SourceSpan,
    pub name: Option<String>,
    pub mode: TestMode,
    pub callback_span: Option<SourceSpan>,
    pub callback_resolution_complete: bool,
    pub assertion_spans: Vec<SourceSpan>,
    pub mock_spans: Vec<SourceSpan>,
    pub body_canonical: Option<String>,
    pub literal_vector: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldFact {
    pub name: String,
    pub span: SourceSpan,
    pub optional: bool,
    pub nullable: bool,
    pub primitive: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFieldTypeFact {
    pub model: TypeKey,
    pub field: String,
    pub span: SourceSpan,
    pub primitive: Resolution<String>,
    pub nullable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelKind {
    Interface,
    TypeAlias,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFact {
    pub key: TypeKey,
    pub span: SourceSpan,
    pub kind: ModelKind,
    pub exported: bool,
    pub fields: Vec<FieldFact>,
    pub extends: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeKey {
    pub path: String,
    pub declaration_start: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportKey {
    pub importer: String,
    pub source: String,
    pub imported: Option<String>,
    pub local: Option<String>,
    pub start_byte: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResolution {
    pub module: Resolution<String>,
    pub export: Resolution<SymbolKey>,
    pub resolved_symbol: Option<SymbolKey>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSurface {
    pub names: BTreeMap<String, SourceSpan>,
    pub default: Option<SourceSpan>,
    pub complete: bool,
    pub unknown_star_reexports: Vec<String>,
    pub common_js_unknown: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OfflineProof {
    CatchRecoveryIsFailure,
    AsyncWithoutAwaitIsMisuse,
    SequentialAwaitIsIndependent,
    TypeResolvedMemberAbsence,
    ExternalExportUnused,
    PropertyOwnershipAndUsage,
    ManifestDependencyUnused,
    CustomAssertionExecution,
    SemanticHelperInterchangeability,
    RequestedScopeIntent,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCoverage {
    pub parse_incomplete_files: BTreeSet<String>,
    pub unresolved_dynamic_imports: BTreeSet<String>,
    pub unresolved_reexports: BTreeSet<String>,
    pub unsupported_tsconfigs: BTreeSet<String>,
    pub external_consumer_unknown_packages: BTreeSet<String>,
    pub unsupported_offline_proofs: BTreeSet<OfflineProof>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFacts {
    pub path: String,
    pub analysis_complete: bool,
    pub is_test: bool,
    pub is_generated: bool,
    pub comments: Vec<CommentFact>,
    pub catches: Vec<CatchFact>,
    pub promise_catches: Vec<PromiseCatchFact>,
    pub casts: Vec<CastFact>,
    pub branches: Vec<BranchFact>,
    pub guards: Vec<GuardFact>,
    pub symbol_types: BTreeMap<SymbolKey, SymbolTypeFact>,
    pub transform_chains: Vec<TransformChainFact>,
    pub async_calls: Vec<AsyncFact>,
    pub calls: Vec<CallFact>,
    pub call_arguments: Vec<CallArgumentFact>,
    pub suites: Vec<SuiteFact>,
    pub setups: Vec<SetupFact>,
    pub test_cases: Vec<TestCaseFact>,
    pub assertions: Vec<AssertionFact>,
    pub mocks: Vec<MockFact>,
    pub production_expressions: Vec<ProductionExpressionFact>,
    pub tests: Vec<TestFact>,
    pub declarations: Vec<DeclarationFact>,
    pub imports: Vec<ImportFact>,
    pub member_uses: Vec<MemberUseFact>,
    pub dynamic_import_spans: Vec<SourceSpan>,
    pub references: Vec<ReferenceFact>,
    pub models: Vec<ModelFact>,
    pub model_field_types: Vec<ModelFieldTypeFact>,
    pub runtime_lines: BTreeSet<u32>,
    pub export_surface: ExportSurface,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarationMetadata {
    pub top_level: bool,
    pub parameter_count: Option<usize>,
    pub capture_count: Option<usize>,
    pub similarity_ignored: bool,
    pub reference_count_is_exact: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReinventionMatch {
    pub candidate: SymbolKey,
    pub existing: SymbolKey,
    pub similarity_millis: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicatedTestGroup {
    pub tests: Vec<TestId>,
    pub literals_only_difference: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectFacts {
    pub root: String,
    pub files: BTreeMap<String, FileFacts>,
    pub entrypoints: BTreeSet<String>,
    pub declaration_metadata: BTreeMap<SymbolKey, DeclarationMetadata>,
    pub reinvention_matches: Vec<ReinventionMatch>,
    pub symbol_uses: BTreeMap<SymbolKey, Vec<SourceSpan>>,
    pub call_targets: BTreeMap<CallSiteKey, Resolution<SymbolKey>>,
    pub imports: BTreeMap<ImportKey, ImportResolution>,
    pub exports: BTreeMap<String, ExportSurface>,
    pub member_uses: BTreeMap<String, Vec<SourceSpan>>,
    pub function_groups: BTreeMap<FunctionDuplicateKey, Vec<FunctionDuplicateKey>>,
    pub type_groups: BTreeMap<TypeKey, Vec<TypeKey>>,
    pub model_field_types: BTreeMap<(TypeKey, String), ModelFieldTypeFact>,
    pub symbol_types: BTreeMap<SymbolKey, SymbolTypeFact>,
    pub suites: BTreeMap<SuiteId, SuiteFact>,
    pub setups: BTreeMap<SetupId, SetupFact>,
    pub tests: BTreeMap<TestId, TestCaseFact>,
    pub assertions: BTreeMap<SourceSpan, AssertionFact>,
    pub mocks: BTreeMap<SourceSpan, MockFact>,
    pub call_arguments: BTreeMap<CallSiteKey, CallArgumentFact>,
    pub call_roles: BTreeMap<CallSiteKey, CallRole>,
    pub production_expressions: BTreeMap<SymbolKey, ProductionExpressionFact>,
    pub coverage: AnalysisCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlopEvidence {
    pub code: String,
    pub label: String,
    pub span: SourceSpan,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlopFinding {
    pub kind: SlopKind,
    pub confidence: SlopConfidence,
    pub span: SourceSpan,
    pub evidence: Vec<SlopEvidence>,
    pub explanation: String,
    pub action: String,
}

pub trait Detector: Send + Sync {
    fn kind(&self) -> SlopKind;
    fn confidence(&self) -> SlopConfidence;
    fn detect(&self, facts: &ProjectFacts) -> Vec<SlopFinding>;
}

#[derive(Debug, Clone, Default)]
pub struct SlopOptions {
    pub include_test_files: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub requested_base: String,
    pub resolved_base: String,
    pub changed_files: usize,
    pub added_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlopReport {
    pub version: u8,
    pub root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<DiffSummary>,
    pub findings: Vec<SlopFinding>,
}
