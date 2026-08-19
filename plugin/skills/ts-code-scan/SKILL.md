---
name: ts-code-scan
description: Offline code search, structural TS/JS analysis, duplicate detection, and project tree overview.
argument-hint: [search pattern]
allowed-tools: Bash, Read
---

## Quick action

If `$ARGUMENTS` is provided, search immediately:

```bash
scanr search "$ARGUMENTS" --json
```

## Commands

### Content and path search

```bash
scanr search "useState" --json
scanr search "todo" --ignore-case --glob '*.ts' --context 2
scanr search --path "components" --json
```

### Tree overview

```bash
scanr tree
scanr tree --path src/commands
scanr tree --depth 4
scanr tree --functions              # compact dotted functions + markers
scanr tree --functions --all-functions
scanr tree --functions --function-details --function-min-lines 3 --function-max-lines 10
scanr tree --health --only-findings --top 20
scanr tree --health --sort-by coupling --json
```

### Structural scan

```bash
scanr scan --mode files
scanr scan --mode folders
scanr scan --file src/api.ts
scanr scan --mode compact
scanr scan --rules hoistable_nested_function,low_value_function,low_value_local_helper
```

### Similarity detection

```bash
scanr dupes --root src
scanr dupes --root src --types
scanr dupes --threshold 0.9 --min-lines 5 --print
```

### Slop review signals

```bash
scanr slop --root .
scanr slop --root . --base HEAD~1
scanr slop --root . --confidence high --json
scanr slop --root . --only scope-inflation,introduced-reinvention
```

Canonical kinds are `suppression-chain`, `swallowed-failure`, `unresolved-api`, `async-misuse`, `dead-surface`, `non-executing-test`, `reinvented-helper`, `low-value-local-helper`, `dominant-container-tiny-helpers`, `redundant-defense`, `one-use-abstraction`, `patch-stack`, `speculative-model`, `comment-inversion`, `parallel-representation`, `generic-name-cluster`, `assertion-monoculture`, `mock-dominated-test`, `duplicated-test-body`, `implementation-mirroring-test`, `scope-inflation`, `introduced-reinvention`, and `generated-surface-burst`. The final three are Medium-confidence diff-only kinds and require `--base`; all other contextual, low-value, and test-theater kinds are Medium, while the first six exact kinds are High.

## Command selection

- Find a literal name or text → `scanr search`
- Find a file by path → `scanr search --path`
- Inspect repository shape → `scanr tree`
- Spot hoistable/capturing/low-value/duplicate functions → `scanr tree --functions`
- Rank evidence-backed code-health hotspots → `scanr tree --health --only-findings --top 20`
- Show a human-readable nested component/function tree with explanations and similarity links → `scanr tree --functions --function-details --function-min-lines 3 --function-max-lines 10`
- List functions, bindings, exports, captures, or violations → `scanr scan`
- Find similar functions or types → `scanr dupes`
- Review explicit slop, reinvention, test-theater, or diff-inflation evidence → `scanr slop`; add `--base <ref>` only for tracked diff evidence

Generic hotspot, complexity, coupling, callback/hook sprawl, size, or dependency-fan-out requests remain on `scanr tree --health`. Do not substitute threshold health metrics when the request specifically asks for slop detector evidence.

For human `slop` output, return the command's one-finding-per-row Markdown table directly. Preserve each row's confidence, evidence, line range, explanation, action, and relevant limitations. Confidence is evidence strength, not probability or AI attribution. With `--base`, say that staged and unstaged tracked content is compared to the resolved commit, untracked files are excluded, and non-diff findings still cover the whole root. Do not call exports “unused”; say no analyzed-project references were resolved.

## Flag reference

**search**: `--root <path>` `--path` `-i|--ignore-case` `--glob <pattern>` `--max-count N` `--context N` `--json`

**tree**: `--root <path>` `--path <subdir>` `--depth N` `--inline N` `--all` `--functions` `--all-functions` `--low-value-max-lines N` `--duplicate-threshold 0.87` `--function-details` `--function-min-lines N` `--function-max-lines N` `--health` `--only-findings` `--top N` `--sort-by severity|coupling|duplicates|size` `--json`

Prefer `--health --only-findings --top 20` for generic hotspots, complexity, coupling, callback sprawl, hook/state/effect sprawl, duplication, oversized files, or dependency fan-out. Treat severity as threshold evidence, not a universal quality score. State the concrete metrics and findings behind any recommendation.

Prefer `--function-details` when the user asks to see files, React components, nested functions, ownership, line counts, or cross-component similarities in one tree. Return the command's plain-English tree directly; do not reconstruct it manually from separate `scan` and `dupes` calls.

Scanner markers are compact internal notation. **Never show them unexplained in a user-facing answer.** Translate them as follows:

| Scanner marker | User-facing wording |
| --- | --- |
| `[H]` | “Can move outside its parent function because it uses no parent variables.” |
| `[C:n]` | “Uses `n` variables from its parent function.” |
| `[L]` | “Small trivial-wrapper candidate; review whether the named function adds value.” |
| `[D:n]` | “Belongs to a similarity group containing `n` functions.” |

Write line counts as numbers under a **Lines** column, never as unexplained shorthand such as `8L`. Present multi-file findings as a Markdown table that humans can scan:

| File | Component | Function | Lines | Why flagged | Suggested action |
| --- | --- | --- | ---: | --- | --- |
| `components/example.tsx` | `Example` | `handleSave` | 4 | Small wrapper using two parent variables | Keep if it clarifies the event; otherwise inline |

Formatting requirements:

- Put exactly one function or finding on each row; never pack multiple functions into one cell with commas, semicolons, or `<br>`.
- Use relative file paths and separate **Component** and **Function** columns.
- Use plain-English findings and actions; do not expose raw scanner markers.
- Sort by file path, then source line, unless the user asks for severity ranking.
- Distinguish findings from recommendations: a short event handler may be intentional and should not automatically be removed.

**scan**: `--root <path>` `--mode compact|verbose|files|folders` `--file <path>` `--include ts,tsx,...` `--exclude <patterns>` `--function-kinds top|top+arrow|top+arrow+class|all` `--rules <rules>` `--include-test-files` `--low-value-max-lines N` `--max-bytes N`

**dupes**: `--root <path>` `--threshold <0-1>` `--min-lines N` `--types` `--print`

**slop**: `--root <path>` `--base <ref>` `--confidence high|medium` `--only <kinds>` `--exclude <kinds>` `--top N` `--include-test-files` `--json`. Filters apply after analysis; `--confidence` is a minimum; exclusion wins over `--only`; Markdown is the default and JSON is schema version 1.
