**Date:** 2026-08-11
**Status:** Pending review
# Detect Code Slop

## Goal

Add a deterministic `scanr slop` command that reports evidence-backed AI-code-slop signatures without claiming authorship or an “AI probability.” It will cover local AST smells, cross-file reinvention and representation drift, test theater, unresolved APIs, dead generated surface, and explicit Git-diff scope inflation while keeping generic health metrics separate under `tree --health`.

## Skills

- `plan-refine` — implementation follows this reviewed execution spec

## References

| Path | Why |
| --- | --- |
| `src/commands/dupes.rs` | cross-file analysis, deterministic grouping, JSON command output |
| `src/commands/scan.rs` | directory/single-file scan orchestration |
| `src/commands/search.rs` | CLI validation, deterministic sorting, human/JSON output split |
| `src/scan/typescript/extract.rs` | shared oxc AST visitor and semantic enrichment |
| `src/scan/health.rs` | neutral AST facts collected during the existing parse |
| `src/scan/rules.rs` | detector trait and file-level violations |
| `src/scan/output.rs` | compact and verbose serialization conventions |
| `src/scan/rules_test.rs` | fixture construction and focused detector assertions |

## Patterns

### Dominant patterns

| Layer | Dominant pattern | Counted exemplars | Ignored deviations |
| --- | --- | --- | --- |
| command | `run(&Args) -> anyhow::Result<()>`, canonicalize root, collect/sort, lock stdout | `commands/scan.rs`, `commands/search.rs`, `commands/dupes.rs` | `tree.rs` accepts decomposed arguments because it predates Args structs |
| AST facts | one oxc parse, `Visit` collector, semantic enrichment after traversal | `typescript/extract.rs`, `scan/health.rs`, `similarity/function_extractor.rs` | source regex is allowed only for comments/directives that are not represented as AST nodes |
| detector | deterministic detector over neutral facts returning structured evidence | `scan/rules.rs`, duplicate grouping in `commands/dupes.rs`, health findings in `commands/tree.rs` | health thresholds remain health signals and do not become slop findings |
| output | serde structs, camelCase verbose JSON, stable path/line/kind sorting | `scan/output.rs`, `commands/dupes.rs`, health JSON in `commands/tree.rs` | compact tuple output remains specific to `scan` |
| tests | colocated unit fixtures plus real parser fixtures, explicit happy/wrong/edge assertions | `scan/rules_test.rs`, `commands/tree.rs` tests, `similarity/tests/function_similarity_test.rs` | — |

### New names

| New name | Kind | Convention it follows | Proving exemplar |
| --- | --- | --- | --- |
| `src/commands/slop.rs` | command file | one file per subcommand | `commands/scan.rs` |
| `SlopArgs` | CLI args struct | `{Command}Args` | `ScanArgs`, `DupesArgs`, `SearchArgs` |
| `SlopConfidence` | clap value enum | typed CLI enum | `HealthSort`, `OutputMode` |
| `src/slop/mod.rs` | analysis module | domain module under `src/` | `scan/mod.rs`, `similarity/mod.rs` |
| `src/slop/types.rs` | report/fact types | domain `types.rs` | `scan/types.rs` |
| `src/slop/facts.rs` | neutral fact extraction | named responsibility file | `similarity/function_extractor.rs` |
| `src/slop/detectors.rs` | detector registry | named responsibility file | `scan/rules.rs` |
| `src/slop/output.rs` | human/JSON output | named responsibility file | `scan/output.rs` |
| `SlopReport` | serialized root report | `{Domain}Report` | `HealthReport` |
| `SlopFinding` | serialized finding | `{Domain}Finding` | `HealthFinding` |
| `SlopEvidence` | source evidence record | finding-owned structured details | `CodeLocation` in `commands/dupes.rs` |
| `SlopKind` | finding category enum | typed finding discriminator | `FunctionKind`, `HealthSeverity` |
| `FileFacts` | per-file neutral facts | `{Scope}Facts` | `HealthAstMetrics` |
| `ProjectFacts` | cross-file indexes | project aggregation before detectors | duplicate entries/groups in `commands/dupes.rs` |
| `DiffScope` | changed-line index | typed base-ref analysis scope | `ScanConfig` |
| `Detector` | detector trait | `Rule` trait | `scan/rules.rs` |
| `analyze` | command analysis boundary | private command analyzer | `analyze` in `commands/dupes.rs` |
| `collect_facts` | AST/semantic extraction entry | verb + result | `extract_file`, `analyze_program` |
| `write_report` | output boundary | `write_result`, `write_matches` | `scan/output.rs`, `commands/search.rs` |
| `src/slop/test_detectors.rs` | test-semantic detectors | named responsibility file | `scan/rules.rs` |
| `src/slop/diff.rs` | Git changed-line extraction | named responsibility file | `commands/search.rs` |
| `src/slop/tests.rs` | core slop tests | colocated test module | `scan/rules_test.rs` |
| `src/slop/tests/contextual.rs` | contextual detector tests | detector-family fixture file | `similarity/tests/function_similarity_test.rs` |
| `src/slop/tests/test_theater.rs` | test-theater tests | detector-family fixture file | `similarity/tests/function_similarity_test.rs` |
| `src/slop/tests/diff.rs` | diff-scope tests | detector-family fixture file | `similarity/tests/function_similarity_test.rs` |
| `build_project_facts` | project aggregation function | verb + result | `build_function_annotations` |
| `test_detectors` | detector registry function | noun + registry | `all_rules` |
| `load_diff_scope` | Git scope loader | verb + result | `collect_files` |
| `SymbolKey` | cross-file symbol key | `{Domain}Key` tuple/newtype | `FunctionDuplicateKey` |
| `ImportKey` | import index key | `{Domain}Key` tuple/newtype | `FunctionDuplicateKey` |
| `TypeKey` | type index key | `{Domain}Key` tuple/newtype | `FunctionDuplicateKey` |
| `ImportResolution` | resolution result | domain result noun | `SimilarityResult` |
| `CommentFact` | comment evidence | `{Node}Fact` | `FunctionInfo` |
| `CatchFact` | catch evidence | `{Node}Fact` | `FunctionInfo` |
| `CastFact` | cast evidence | `{Node}Fact` | `FunctionInfo` |
| `AsyncFact` | async evidence | `{Node}Fact` | `FunctionInfo` |
| `TestFact` | test evidence | `{Node}Fact` | `FunctionInfo` |
| `DeclarationFact` | declaration evidence | `{Node}Fact` | `FunctionInfo` |
| `ImportFact` | import evidence | `{Node}Fact` | `FunctionInfo` |
| `ModelFact` | type/model evidence | `{Node}Fact` | `FunctionInfo` |
| `SuppressionChain` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::High` |
| `SwallowedFailure` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::High` |
| `UnresolvedApi` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::High` |
| `AsyncMisuse` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::High` |
| `DeadSurface` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::High` |
| `NonExecutingTest` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::High` |
| `ReinventedHelper` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `RedundantDefense` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `FunctionRole` | function classification enum | typed function metadata | `FunctionKind` |
| `ClassInfo` | class span/ownership record | `{Node}Info` | `FunctionInfo` |
| `LowValueLocalHelper` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `DominantContainerTinyHelpers` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `OneUseAbstraction` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `PatchStack` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `SpeculativeModel` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `CommentInversion` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `ParallelRepresentation` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `GenericNameCluster` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `AssertionMonoculture` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `MockDominatedTest` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `DuplicatedTestBody` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `ImplementationMirroringTest` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `ScopeInflation` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |
| `IntroducedReinvention` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::High` |
| `GeneratedSurfaceBurst` | `SlopKind` variant | PascalCase evidence category | `HealthSeverity::Medium` |

## Acceptance

- `scanr slop` emits deterministic findings with kind, confidence, file, span, evidence, explanation, and action; it never labels code as AI-generated.
- High-confidence detection covers duplicate reinvention, suppressions/cast chains, swallowed failures, unresolved APIs, async misuse, dead generated surface, and non-executing tests.
- Medium-confidence detection covers role-aware low-value local helpers, dominant functions/classes with tiny satellite helpers, redundant guards, one-use abstractions, patch stacking, speculative models, comment inversion, test theater, parallel representations, and generic-name clusters.
- `--base <ref>` limits scope-based findings to added lines and distinguishes newly introduced helpers from pre-existing equivalents.
- Human output is a one-finding-per-row Markdown table; JSON is versioned and stable; `--confidence`, `--only`, `--exclude`, and `--top` filter after analysis.
- Existing `search`, `scan`, `dupes`, compact tree, detailed tree, and health output remain byte-for-byte stable on existing fixtures.

## Phases

- [ ] Calibrate low-value local helpers
- [ ] Add slop contracts and command shell
- [ ] Collect neutral file facts
- [ ] Build cross-file project facts
- [ ] Add exact local detectors
- [ ] Add contextual slop detectors
- [ ] Detect test theater
- [ ] Add diff-aware scope findings
- [ ] Render and document slop reports
- [ ] Calibrate against comma
- [ ] Lint + typecheck

## Decisions

- Report “slop-like evidence,” never AI authorship or probability, because 2026 community reports emphasize false confidence and ownership rather than a reliable AI fingerprint.
- Keep `tree --health` as a separate messiness report; file size, complexity, hooks, and dependency counts are context but are not slop findings by themselves.
- Assign high confidence only when AST, semantic references, module resolution, control flow, or Git-added-line evidence proves the condition.
- Assign medium confidence to intent-sensitive patterns and require at least two independent signals before emitting one-use, speculative-model, comment-inversion, or test-theater findings.
- Resolve local modules and installed package declarations offline; do not query registries, documentation services, or models.
- Use Git only for explicit `--base`; directory analysis remains filesystem-only and deterministic.
- Calibrate `LowValueLocalHelper` first through the existing `scan --rules` surface so real-project evidence fixes its gates before the broader slop command consumes it.

## Change log

- 2026-08-11 — User approved a focused `LowValueLocalHelper` calibration before the broader slop command; added Phase 0 and retained the final detector in Phase 5.
- 2026-08-11 — First comma run produced five candidates; tightened trivial-call extraction to reject transformed call chains and computed map lookups, leaving two direct wrappers (`newId`, `formatYearTick`).
- 2026-08-11 — User approved TSX-aware function roles and a configurable dominant function/class plus tiny satellite-helper detector for focused calibration.
- 2026-08-11 — AST-backed JSX attribution plus naming/ownership classified comma functions; the 300/3/2 dominant rule found `ChartTableBody` (330 lines) with `isLeaf` and `isZeroAmount`, retained as a medium-confidence review signal because both predicates may be intentional.
- 2026-08-11 — User selected an opt-in balanced loose profile: retain 3-line, 1–2-reference, 300-line-container defaults while including ordinary small bodies and component-local TSX functions; comma expands to 38 functions and 4 groups.
- 2026-08-12 — Completed all 23 slop kinds and calibrated `comma/apps/web`: 54 findings in 1.07 seconds (46 high, 8 medium), deterministic Markdown/JSON, plus one diff-only generated-surface-burst finding for `HEAD~1`; suppressed seven observed false-positive classes.

## Implementation rules

- Build plumbing first: CLI args, serialized contracts, neutral fact types, and an empty detector registry before detector behavior.
- Keep phases dependency-ordered; each phase should be independently assignable after its listed dependency is complete.
- Update this plan when implementation evidence changes the approach: edit later phase file tables, final shapes, tests, and decisions before continuing.
- Before reporting done, dry-run the implemented flow by reading final files and proving every command, fact, detector, report field, test, and validation command exists.
- Preserve all currently uncommitted detailed-tree, health-mode, README, and skill changes; do not commit without explicit approval.

## Phase 0 — Calibrate low-value local helpers

**Depends on:** None

**Parallel:** No, calibration results define the final detector gate

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/scan/types.rs` | add `FunctionRole`, JSX/ownership metadata, and `ClassInfo` | role and container evidence |
| `src/scan/typescript/extract.rs` | classify JSX-bearing components, hooks, component-local functions, helpers, classes, and class methods | AST-backed roles |
| `src/scan/rules.rs` | add `low_value_local_helper` and `dominant_container_tiny_helpers` | focused detector surface |
| `src/scan/rules_test.rs` | cover role, usage, dominant function/class, helper-count, and line-threshold boundaries | false-positive control |
| `README.md` | document the calibration rule and evidence fields | command discoverability |
| `docs/real-world-tests.md` | record comma command, count, and reviewed matches | durable calibration |

**Final shape:**

Mirrors `LowValueFunction` in `scan/rules.rs`:
```rust
struct LowValueLocalHelper {
    include_test_files: bool,
    max_lines: u32,
    max_references: usize,
    loose: bool,
}

struct DominantContainerTinyHelpers {
    include_test_files: bool,
    container_min_lines: u32,
    helper_max_lines: u32,
    helper_min_count: usize,
    max_references: usize,
}
```

```rust
function.parent.is_none()
    && !function.exported
    && function.low_value_reason.is_some()
    && function_lines(function) <= max_lines
    && (1..=max_references).contains(&binding.refs)
```

The violation detail is `name:reason:references`, mirroring the existing `name:reason` detail shape while adding the evidence that distinguishes this rule.

**Tests:**

- Happy: non-exported top-level three-line direct-call helpers with one and two same-file references are reported with `helper` role; a 300-line function or class plus two qualifying helpers emits one grouped dominant-container finding.
- Wrong: React components, hooks, nested React handlers, exported helpers, zero-reference dead code, three-reference helpers, non-trivial bodies, and large containers with fewer than two helpers are not reported.
- Edge: same-name bindings on different lines resolve by declaration line; TSX top-level helpers remain `helper`, JSX-returning uppercase functions become `reactComponent`, and class methods retain their owning class.

**Dry run:**

- Verify the focused role, JSX-attribution, standalone-helper, dominant-function, and dominant-class tests.
- Run both focused rules on comma at container/helper/count thresholds `300/3/2` and manually review every match in a one-finding-per-row table.
- Preserve standalone strictness: only `newId` and `formatYearTick` qualify; keep the grouped `ChartTableBody` predicate finding explicitly medium-confidence.
- Update the final Phase 5 gate when later corpus evidence changes the role, line, reference, nesting, export, or helper-count criteria.

## Phase 1 — Add slop contracts and command shell

**Depends on:** None

**Parallel:** No, establishes contracts consumed by every later phase

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/cli.rs` | add `SlopArgs`, `SlopConfidence`, and `Commands::Slop` | typed CLI contract |
| `src/main.rs` | route `Commands::Slop(args)` | command dispatch |
| `src/commands/mod.rs` | export `slop` | command registration |
| `src/commands/slop.rs` | create validation and empty analysis shell | command ownership |
| `src/slop/mod.rs` | create module exports | analysis boundary |
| `src/slop/types.rs` | create report, finding, evidence, fact, and detector types | stable contracts |
| `Cargo.toml` | retain current dependencies | no new runtime dependency |

**Final shape:**

Current (`src/main.rs`):
```rust
match cli.command {
    Commands::Dupes(args) => commands::dupes::run(&args),
    Commands::Search(args) => commands::search::run(&args),
```

Final, mirroring `Commands::Dupes`:
```rust
match cli.command {
    Commands::Dupes(args) => commands::dupes::run(&args),
    Commands::Slop(args) => commands::slop::run(&args),
    Commands::Search(args) => commands::search::run(&args),
```

Mirrors `DupesArgs` and `HealthSort`:
```rust
pub struct SlopArgs {
    pub root: String,
    pub base: Option<String>,
    pub confidence: SlopConfidence,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
    pub top: Option<usize>,
    pub include_test_files: bool,
    pub json: bool,
}
```

Mirrors `HealthFinding` and `CodeLocation`:
```rust
pub struct SlopFinding {
    pub kind: SlopKind,
    pub confidence: SlopConfidence,
    pub evidence: Vec<SlopEvidence>,
    pub explanation: String,
    pub action: String,
}
```

**Tests:**

- Happy: `scanr slop --root <fixture> --json` emits a versioned empty report before detectors are registered.
- Wrong: unknown `--only`/`--exclude` detector names fail with the valid names.
- Edge: `--top 0` emits a valid report with zero findings.

**Dry run:**

- Run `scanr slop --help` and verify every accepted flag and confidence value.
- Read `main.rs`, `commands/mod.rs`, and `slop/mod.rs` and verify one dispatch path.

## Phase 2 — Collect neutral file facts

**Depends on:** Phase 1 — Add slop contracts and command shell

**Parallel:** No, establishes facts consumed by detectors

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/slop/facts.rs` | implement one-pass oxc visitor and comment/directive extraction | neutral evidence |
| `src/slop/types.rs` | define fact records and spans | typed detector input |
| `src/scan/typescript/parse.rs` | invoke `collect_facts` on the existing parsed program | preserve one-parse architecture |
| `src/scan/types.rs` | carry slop facts with `FileIndex` | shared scan result |
| `src/scan/typescript/parse_test.rs` | cover shared parse and fact production | regression coverage |

**Final shape:**

Current (`parse.rs`):
```rust
let result = extract::extract_file(&parser_ret.program, &semantic, &source, filter);
```

Final, mirroring `analyze_program` in `scan/health.rs`:
```rust
let result = extract::extract_file(&parser_ret.program, &semantic, &source, filter);
let slop = collect_facts(&parser_ret.program, &semantic, &source);
```

Mirrors `HealthAstMetrics`:
```rust
pub struct FileFacts {
    pub comments: Vec<CommentFact>,
    pub catches: Vec<CatchFact>,
    pub casts: Vec<CastFact>,
    pub async_calls: Vec<AsyncFact>,
    pub tests: Vec<TestFact>,
    pub declarations: Vec<DeclarationFact>,
    pub imports: Vec<ImportFact>,
    pub models: Vec<ModelFact>,
}
```

**Tests:**

- Happy: one TSX fixture records spans for catches, assertions, casts, suppressions, comments, imports, functions, interfaces, and async calls.
- Wrong: comments and string literals containing code-like words create no AST facts.
- Edge: malformed-but-recoverable TSX preserves parse diagnostics and emits facts only for valid nodes.

**Dry run:**

- Parse one fixture through `process_file_with_similarity` and verify scan, similarity, health, and slop facts came from one parser invocation.
- Confirm every fact stores a stable relative path plus one-based line range.

## Phase 3 — Build cross-file project facts

**Depends on:** Phase 2 — Collect neutral file facts

**Parallel:** No, contextual detectors consume this index

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/slop/mod.rs` | aggregate files into `ProjectFacts` | analysis orchestration |
| `src/slop/types.rs` | add symbol, import, type, duplicate, and usage indexes | cross-file evidence |
| `src/slop/facts.rs` | resolve relative imports, tsconfig aliases, installed declarations, and exported names | unresolved API evidence |
| `src/commands/dupes.rs` | expose deterministic function/type groups without serializing command-private structs | reuse similarity engine |
| `src/slop/tests.rs` | add multi-file and package fixture graphs | index coverage |

**Final shape:**

Mirrors `function_duplicate_groups`:
```rust
pub struct ProjectFacts {
    pub files: BTreeMap<String, FileFacts>,
    pub symbol_uses: BTreeMap<SymbolKey, Vec<SourceSpan>>,
    pub imports: BTreeMap<ImportKey, ImportResolution>,
    pub function_groups: BTreeMap<FunctionDuplicateKey, Vec<FunctionDuplicateKey>>,
    pub type_groups: BTreeMap<TypeKey, Vec<TypeKey>>,
}
```

Mirrors `collect_files` + sorted duplicate groups:
```rust
pub fn build_project_facts(files: Vec<ParsedFile>) -> Result<ProjectFacts> {
    let mut facts = ProjectFacts::from_files(files)?;
    facts.resolve_imports()?;
    facts.index_symbol_uses();
    facts.group_similar_definitions()?;
    Ok(facts)
}
```

**Tests:**

- Happy: relative, index, extensionless, tsconfig-alias, and installed-package imports resolve to exact files/exports.
- Wrong: missing relative modules and missing named exports remain unresolved with the attempted target recorded.
- Edge: cycles terminate because resolution uses visited canonical paths.

**Dry run:**

- Resolve imports in a fixture containing `.ts`, `.tsx`, `index.ts`, alias, and package declaration targets.
- Print the internal index in a test and verify all keys sort by path, line, then symbol.

## Phase 4 — Add exact local detectors

**Depends on:** Phase 3 — Build cross-file project facts

**Parallel:** Yes, detector implementations touch only `detectors.rs` and detector fixtures

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/slop/detectors.rs` | register high-confidence local detectors | exact slop evidence |
| `src/slop/types.rs` | add exact `SlopKind` variants | stable output kinds |
| `src/slop/tests.rs` | add detector fixtures | behavior coverage |

**Final shape:**

Mirrors `Rule` in `scan/rules.rs`:
```rust
pub trait Detector: Send + Sync {
    fn kind(&self) -> SlopKind;
    fn confidence(&self) -> SlopConfidence;
    fn detect(&self, facts: &ProjectFacts) -> Vec<SlopFinding>;
}
```

Detector matrix:
| `SlopKind` | Exact evidence |
| --- | --- |
| `SuppressionChain` | `as any`, double assertions, non-null assertions, `@ts-ignore`, or lint-disable clustered in one expression/function |
| `SwallowedFailure` | empty catch, log-only catch, catch returning success/default, or promise catch without throw/reject |
| `UnresolvedApi` | unresolved local/package import, missing named export, or member absent from installed declaration |
| `AsyncMisuse` | async function without await, floating call to known async function, or sequential await directly in a loop |
| `DeadSurface` | unreferenced non-entry export, unused prop/config field, unused dependency, or permanent placeholder branch |
| `NonExecutingTest` | no reachable assertion, unconditional return before assertions, skipped-only body, or assertion callback never invoked |

**Tests:**

- Happy: each matrix row has one minimal fixture that emits one finding with exact span evidence.
- Wrong: intentional `void promise`, rethrowing catches, test callbacks passed to runners, and entry-point exports emit nothing.
- Edge: one function with several suppressions emits one grouped finding rather than line spam.

**Dry run:**

- Run the exact-detector fixture suite and inspect every finding’s source span and plain-English action.
- Verify high-confidence findings contain no threshold-only evidence.

## Phase 5 — Add contextual slop detectors

**Depends on:** Phase 3 — Build cross-file project facts

**Parallel:** Yes, after Phase 3; does not edit Phase 4 fixture files

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/slop/detectors.rs` | register medium-confidence contextual detectors | intent-sensitive evidence |
| `src/slop/types.rs` | add contextual `SlopKind` variants | stable output kinds |
| `src/slop/tests/contextual.rs` | create contextual fixture suite | isolate heuristic tests |

**Final shape:**

Detector matrix:
| `SlopKind` | Required combined evidence |
| --- | --- |
| `ReinventedHelper` | structural similarity plus an existing referenced helper outside the candidate file |
| `LowValueLocalHelper` | non-exported named function is at most three lines, has one or two same-file references and no cross-file references, and its AST is empty, a constant/identity/property return, or a direct pass-through call |
| `RedundantDefense` | runtime guard/cast plus a non-null typed or validator-proven value in the same flow |
| `OneUseAbstraction` | interface/factory/wrapper has one implementation/caller and only forwards data/calls |
| `PatchStack` | three or more consecutive normalize/cast/default/adapter operations on one value |
| `SpeculativeModel` | omittable-or-nullable field ratio at least 60% plus at least half the fields unused outside declaration/serialization |
| `CommentInversion` | obvious statements have narrating comments plus the file’s highest-complexity function lacks intent comments |
| `ParallelRepresentation` | types/DTOs share at least 85% normalized fields but differ in optionality or primitive type |
| `GenericNameCluster` | at least four generic names in one file and at least two are high-coupling or high-complexity functions |

**Tests:**

- Happy: each detector requires both signals and reports both in `evidence`.
- Wrong: React handlers passed as JSX/event callbacks, predicate/type-guard helpers, public adapter boundaries, intentionally sparse patch DTOs, and generated declaration files emit nothing.
- Edge: a three-line direct-call helper with two references emits one medium-confidence finding; three references suppress it, and near-duplicate DTOs with identical intentional aliases collapse to one finding.

**Dry run:**

- Remove either signal from each fixture and verify the finding disappears.
- Verify contextual findings serialize with `confidence: medium`.

## Phase 6 — Detect test theater

**Depends on:** Phase 3 — Build cross-file project facts

**Parallel:** Yes, after Phase 3; owns test-specific detector module

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/slop/test_detectors.rs` | implement test-theater detectors | separate test semantics |
| `src/slop/mod.rs` | register test detectors | analysis wiring |
| `src/slop/types.rs` | add test metrics and kinds | report contract |
| `src/slop/tests/test_theater.rs` | create representative test suites | false-positive control |

**Final shape:**

Mirrors detector registry in `scan/rules.rs`:
```rust
pub fn test_detectors() -> Vec<Box<dyn Detector>> {
    vec![
        Box::new(AssertionMonoculture),
        Box::new(MockDominatedTest),
        Box::new(DuplicatedTestBody),
        Box::new(ImplementationMirroringTest),
    ]
}
```

Emission rules:
| Kind | Evidence gate |
| --- | --- |
| `AssertionMonoculture` | at least six tests, at least 80% use one assertion shape, and no boundary/error assertion exists |
| `MockDominatedTest` | mocks/stubs outnumber SUT calls and assertions combined across at least four tests |
| `DuplicatedTestBody` | normalized test bodies form a similarity group of at least four with literals as the only material change |
| `ImplementationMirroringTest` | assertion reproduces the same expression/callee chain as production instead of checking observable output |

**Tests:**

- Happy: generated CRUD null-case suites trigger assertion monoculture and duplicated bodies.
- Wrong: table-driven tests, property tests, snapshots, and intentional parameterized cases emit nothing.
- Edge: a suite with many repetitive tests but distinct failure boundaries remains below the combined-evidence gate.

**Dry run:**

- Run fixtures and compare reported test names, assertion counts, mock counts, and normalized body fingerprints.
- Confirm individual trivial tests do not trigger file-level theater findings.

## Phase 7 — Add diff-aware scope findings

**Depends on:** Phase 4 — Add exact local detectors; Phase 5 — Add contextual slop detectors; Phase 6 — Detect test theater

**Parallel:** No, combines all detector output with changed-line scope

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/slop/diff.rs` | parse `git diff --unified=0` into `DiffScope` | added-line evidence |
| `src/commands/slop.rs` | invoke Git only when `--base` is provided | explicit scope mode |
| `src/slop/detectors.rs` | add scope inflation and introduced-reinvention findings | diff-aware slop |
| `src/slop/types.rs` | add diff summary fields | JSON evidence |
| `src/slop/tests/diff.rs` | create temporary Git fixture | deterministic diff tests |

**Final shape:**

Mirrors explicit command validation in `commands/dupes.rs`:
```rust
pub fn load_diff_scope(root: &Path, base: &str) -> Result<DiffScope> {
    let output = Command::new("git")
        .args(["diff", "--unified=0", "--no-color", base, "--"])
        .current_dir(root)
        .output()?;
    DiffScope::parse(&output.stdout)
}
```

Diff findings:
| Kind | Evidence |
| --- | --- |
| `ScopeInflation` | added behavior under 20 lines accompanied by at least three new helpers/files or a new dependency |
| `IntroducedReinvention` | added helper is similar to a pre-existing helper outside added lines |
| `GeneratedSurfaceBurst` | one diff adds at least three unused exports/options/types or four near-identical tests |

**Tests:**

- Happy: a small requested behavior represented by a fixture diff with helper/file/dependency inflation emits one grouped finding.
- Wrong: a large cohesive feature diff and generated migration fixture emit nothing.
- Edge: renamed files and deleted lines do not count as added surface.

**Dry run:**

- Run `scanr slop --base HEAD~1 --json` in the temporary Git fixture and verify every finding evidence intersects an added line.
- Run without `--base` and verify scope-only kinds are absent.

## Phase 8 — Render and document slop reports

**Depends on:** Phase 7 — Add diff-aware scope findings

**Parallel:** No, finalizes public output

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/slop/output.rs` | implement Markdown table and JSON writers | human/machine output |
| `src/commands/slop.rs` | filter, sort, limit, and write | command completion |
| `README.md` | document semantics, flags, kinds, confidence, schema | public API |
| `plugin/skills/search/SKILL.md` | route slop requests and require plain-language tables | agent behavior |
| `docs/real-world-tests.md` | add slop calibration checklist | repeatable validation |

**Final shape:**

Mirrors the skill’s existing one-finding-per-row requirement:
```text
| File | Finding | Confidence | Evidence | Why it matters | Suggested action |
| --- | --- | --- | --- | --- | --- |
| `src/payments.ts:44` | Swallowed failure | High | catch logs and returns success | payment failure appears successful | rethrow or return an explicit failure |
```

Mirrors `DupesOutput` versioned JSON:
```json
{
  "version": 1,
  "root": "/project",
  "findings": [
    {
      "kind": "swallowedFailure",
      "confidence": "high",
      "evidence": [{ "path": "src/payments.ts", "line": 44 }]
    }
  ]
}
```

**Tests:**

- Happy: human output contains one finding per row with no raw enum shorthand; JSON round-trips through `serde_json`.
- Wrong: unknown detector filters fail before scanning.
- Edge: evidence text containing pipes/newlines is escaped in Markdown.

**Dry run:**

- Generate human and JSON output twice and compare bytes.
- Verify every documented kind appears in `scanr slop --help` or README detector reference.

## Phase 9 — Calibrate against comma

**Depends on:** Phase 8 — Render and document slop reports

**Parallel:** No, calibration can change detector gates

**Files:**
| Path | Change | Why |
| --- | --- | --- |
| `src/slop/tests/contextual.rs` | add minimized regressions from reviewed comma findings | preserve calibration |
| `src/slop/tests/test_theater.rs` | add minimized real test patterns | false-positive control |
| `docs/real-world-tests.md` | record command, counts, runtime, and reviewed examples | durable evidence |

**Final shape:**

Mirrors existing real-world deterministic validation:
```bash
scanr slop --root /Users/jon/projects/comma/apps/web/src --confidence high --json
scanr slop --root /Users/jon/projects/comma/apps/web/src --only reinvented-helper,test-theater --top 20
scanr slop --root /Users/jon/projects/comma --base HEAD~1 --json
```

**Tests:**

- Happy: manually inspect at least five findings per detector family and record true examples.
- Wrong: manually inspect at least five non-findings for one-use handlers, intentional guards, table tests, DTOs, and comments.
- Edge: test directories remain excluded unless `--include-test-files`, except test-theater analysis which scans tests as its required subject.

**Dry run:**

- Compare two complete JSON runs byte-for-byte.
- Confirm high-confidence output contains no threshold-only health findings.
- Confirm runtime remains under five seconds for `apps/web` on the current machine. Measured 1.07 seconds on 2026-08-12.

## Phase 10 — Lint + typecheck

**Dry run:**

- Read `cli.rs`, `main.rs`, `commands/slop.rs`, every `src/slop/` file, README, skill, and real-world checklist; verify every planned flag, kind, evidence field, detector, and command exists.
- Run focused slop fixtures and the real `comma` human/JSON commands.
- Compare existing compact and detailed tree fixture output to pre-slop snapshots.

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
git diff --check
```
