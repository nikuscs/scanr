# Real-world test checklist

## CLI

- [ ] `scanr --help` lists `search`, `scan`, `tree`, and `dupes`
- [ ] `scanr search --help` documents content/path search flags
- [ ] `scanr scan --help` documents output modes and rules
- [ ] `scanr tree --help` documents tree controls
- [ ] `scanr dupes --help` documents similarity controls
- [ ] `scanr slop --help` documents root, explicit base, confidence, filters, top, test inclusion, and JSON controls

## Search

- [ ] Content results match `rg -n` for the same literal pattern
- [ ] JSON output is stable and sorted by path then line
- [ ] Path mode respects ignored files
- [ ] `--ignore-case`, `--glob`, `--max-count`, and `--context` work together

## Scan

- [ ] TypeScript and TSX files parse without panics
- [ ] Functions, bindings, references, and exports remain stable
- [ ] Compact, verbose, files, and folders modes serialize deterministically
- [ ] Rules report expected violations
- [ ] Nested functions expose parent and capture information
- [ ] Low-value findings are limited to conservative trivial shapes and the configured line limit
- [ ] `low_value_local_helper` reports only non-exported top-level ordinary helpers with one or two same-file references; manually inspect every match
- [ ] Verbose output classifies JSX-bearing components, hooks, component-local functions, class methods, and ordinary helpers
- [ ] `dominant_container_tiny_helpers` groups one 300-or-more-line function/class with at least two configured tiny 1–2-use helpers
- [ ] `--loose-low-value` includes ordinary and component-local small bodies without including components, hooks, class methods, tests, exports, dead code, or functions with more than two references

### Comma calibration — 2026-08-11

```bash
scanr scan --root /Users/jon/projects/comma/apps/web/src \
  --rules low_value_local_helper,dominant_container_tiny_helpers \
  --low-value-max-lines 3 \
  --dominant-container-min-lines 300 \
  --dominant-helper-min-count 2 \
  --mode verbose
```

- Two standalone helper findings: `newId` (3 lines, 2 references) and `formatYearTick` (3 lines, 1 reference).
- One grouped imbalance finding: `ChartTableBody` (330 lines) with `isLeaf` (1 line, 2 references) and `isZeroAmount` (3 lines, 2 references).
- The grouped finding is a medium-confidence review signal, not proof of low value: both predicate names encode domain meaning and may be intentional.
- Function roles: 470 React components, 149 hooks, 1,691 component-local functions, 19 class methods, and 1,913 ordinary helpers.
- Balanced loose profile (`--loose-low-value` at the default 3-line and 300-line thresholds): 38 small low-use functions and 4 dominant-container groups after excluding tests.

## Tree

- [ ] Ignored and hidden directories are omitted
- [ ] Test paths are omitted by default and included with `--all`
- [ ] Natural sorting and depth collapsing remain stable
- [ ] `--functions` emits stable dotted names and `[H]`, `[C:n]`, `[L]`, `[D:n]` markers
- [ ] `--all-functions` expands callbacks otherwise summarized as `+N anonymous`

## Similarity

- [ ] Function and type comparisons match the upstream regression fixtures
- [ ] Results are deterministic across runs

## Slop review flow

The 23 canonical kinds are `suppression-chain`, `swallowed-failure`, `unresolved-api`, `async-misuse`, `dead-surface`, `non-executing-test`, `reinvented-helper`, `low-value-local-helper`, `dominant-container-tiny-helpers`, `redundant-defense`, `one-use-abstraction`, `patch-stack`, `speculative-model`, `comment-inversion`, `parallel-representation`, `generic-name-cluster`, `assertion-monoculture`, `mock-dominated-test`, `duplicated-test-body`, `implementation-mirroring-test`, `scope-inflation`, `introduced-reinvention`, and `generated-surface-burst`. The first six exact kinds are High confidence. Every other kind is Medium; the final three are diff-only and require `--base`.

Confidence records offline evidence strength, not probability, precision, quality score, AI authorship, or semantic interchangeability. Generated/test/entrypoint exclusions are conservative. Dynamic/reflection/string consumers, unresolved analysis, external package/plugin consumers, and unavailable exact property ownership limit absence claims. `--base` includes tracked staged and unstaged changes relative to the resolved commit but excludes untracked files and cannot infer requested product scope.

- [ ] Run `scanr slop --help` and confirm every public flag and default is accurate
- [ ] Run `scanr slop --root <non-git-temp-project>` successfully; no-base analysis must not require or invoke Git and must emit no diff-only kind
- [ ] In a temporary Git repository, commit a base, then cover modified, added, renamed-plus-edited, deleted, binary, Unicode/space path, dependency-addition, malformed-manifest, and untracked-file cases with `scanr slop --root . --base HEAD`
- [ ] Confirm a missing ref and a non-worktree `--base HEAD` fail rather than producing an empty diff
- [ ] Confirm `scope-inflation`, `introduced-reinvention`, and `generated-surface-burst` appear only with explicit `--base`
- [ ] Confirm non-diff findings remain whole-root findings when `--base` is present
- [ ] Exercise repeated/comma-delimited `--only` and `--exclude`, overlap where exclusion wins, `--confidence high`, `--confidence medium`, `--top 0`, and a positive `--top`
- [ ] Confirm Markdown has the exact six-column header, one finding per row, plain-English names, full line ranges, escaped pipes/newlines, and no raw enum/debug names
- [ ] Confirm JSON is compact schema version 1, has only relative finding/evidence paths, and includes a diff summary only with `--base`
- [ ] Manually review true and false positives for every detector family; do not infer precision/recall from fixtures

Repeat both formats and compare bytes:

```bash
scanr slop --root <fixture> > /tmp/slop-1.md
scanr slop --root <fixture> > /tmp/slop-2.md
cmp /tmp/slop-1.md /tmp/slop-2.md
scanr slop --root <fixture> --base HEAD --json > /tmp/slop-1.json
scanr slop --root <fixture> --base HEAD --json > /tmp/slop-2.json
cmp /tmp/slop-1.json /tmp/slop-2.json
```

### Comma slop calibration workflow

Run only after the unit, temporary-Git, and deterministic-byte gates pass. Record reviewed examples and non-examples before adding counts, runtime, or precision claims:

```bash
scanr slop --root /Users/jon/projects/comma/apps/web --confidence high --json
scanr slop --root /Users/jon/projects/comma/apps/web \
  --only reinvented-helper,assertion-monoculture,mock-dominated-test,duplicated-test-body,implementation-mirroring-test \
  --top 20
scanr slop --root /Users/jon/projects/comma --base HEAD~1 --json
```

Calibration recorded 2026-08-12 with the release binary:

- Whole-project command: `scanr slop --root /Users/jon/projects/comma/apps/web --json`
- 54 findings in 1.07 seconds: 30 suppression chains, 16 swallowed failures, 4 parallel representations, 2 low-value local helpers, 1 dominant-container group, and 1 one-use abstraction.
- Confidence split: 46 high and 8 medium. Human and JSON output were each byte-identical across two complete runs.
- Diff command: `scanr slop --root /Users/jon/projects/comma --base HEAD~1 --json`; 55 changed files, 7,422 added lines, and 1 generated-surface-burst finding in 1.17 seconds.
- Calibration removed false positives from unresolved hoisted packages, type-only and generated imports, public/Props DTOs, member-property guards, handled promise chains, test-only ordinary findings, and overlapping helper findings.

- [x] Review at least five findings per detector family where available
- [x] Review at least five intentional non-findings for one-use handlers, guards, table/property/snapshot tests, DTOs, entrypoints, generated code, and comments
- [x] Save the exact commands, revision, counts, elapsed time, and reviewed examples only after actually running them
- [x] Compare two complete JSON runs byte-for-byte before recording calibration results

## Offline guarantee

- [ ] The binary performs no network calls
- [ ] No database or service is required
