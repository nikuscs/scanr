# 📡 scanr

[![Release](https://img.shields.io/github/v/release/nikuscs/scanr)](https://github.com/nikuscs/scanr/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A fast, deterministic static-analysis and search CLI for TypeScript and JavaScript projects. `scanr` is a single local binary: it needs no database, network access, service, or API key.

## Features

- **Structural analysis** — extract functions, captures, bindings, references, exports, and rule violations with oxc
- **Content and path search** — gitignore-aware, parallel grep with stable JSON output
- **Similarity detection** — APTED-based function and type comparison, including type literals
- **Slop review signals** — evidence-backed exact, contextual, test-theater, and opt-in diff findings
- **Project tree** — compact repository structure for quick orientation
- **Agent-friendly** — deterministic output suitable for scripts and coding agents

## Install

```bash
cargo install --git https://github.com/nikuscs/scanr
```

Or build from source with Rust 1.95+:

```bash
git clone https://github.com/nikuscs/scanr
cd scanr
cargo build --release
```

## Quick start

```bash
scanr search "useState" --root . --json
scanr scan --root . --mode files
scanr dupes --root . --types
scanr slop --root .
scanr tree --root .
```

## Commands

### `scanr search`

Search file contents with a literal pattern:

```bash
scanr search "useState"
scanr search "todo" --ignore-case --glob '*.ts' --context 2
scanr search "handler" --max-count 5 --json
```

Search paths instead of contents:

```bash
scanr search --path "components"
scanr search --path '*.test.ts'
```

The walker respects `.gitignore`. Results are sorted by path and line, including when files are searched in parallel.

Search flags:

- `--root <path>` — search root (default `.`)
- `--path <pattern>` — search paths instead of contents; accepts substrings, `*`, and `?`
- `-i, --ignore-case` — ASCII case-insensitive matching
- `--glob <pattern>` — gitignore-style path filter; repeatable
- `--max-count <n>` — maximum matching lines per file
- `--context <n>` — lines before and after each match (default `0`)
- `--json` — emit `[{path,line,text,before,after}]`

### `scanr scan`

Extract TypeScript and JavaScript structure:

```bash
scanr scan
scanr scan --root apps/web --mode verbose
scanr scan --file src/index.ts
scanr scan --mode files
scanr scan --mode folders
scanr scan --rules hoistable_nested_function
```

Flags:

- `--root <path>` — project root
- `--mode compact|verbose|files|folders` — output shape
- `--include ts,tsx,js,jsx,...` — included extensions
- `--exclude <patterns>` — excluded paths
- `--max-bytes <bytes>` — maximum file size
- `--function-kinds top|top+arrow|top+arrow+class|all` — function categories
- `--rules <names>` — run selected rules (all rules run by default)
- `--include-test-files` — include tests in scope and low-value checks
- `--low-value-max-lines <n>` — maximum tiny-helper length (default `3`)
- `--loose-low-value` — include ordinary small bodies and component-local TSX functions while retaining the 1–2-reference and role exclusions
- `--dominant-container-min-lines <n>` — minimum dominant function/class length (default `300`)
- `--dominant-helper-min-count <n>` — required tiny low-use helpers beside it (default `2`)
- `--file <path>` — scan one file

Built-in rules are `no_unused_bindings`, `one_exported_function_per_file`, `max_functions_per_file`, `hoistable_nested_function`, `low_value_function`, `low_value_local_helper`, `dominant_container_tiny_helpers`, and `satellite_cluster`. `satellite_cluster` emits one grouped scan violation when a file has at least two non-exported file-scope helpers of at most eight lines with one or two same-file references; non-trivial helpers must be single-use. The low-value rules are deliberately narrow: trivial bodies are empty, constant/identity/direct-property returns, or direct pass-through calls. `low_value_local_helper` further requires a non-exported top-level ordinary helper with one or two same-file references and excludes React components, hooks, component-local functions, class methods, dead code, nested functions, exports, transformed calls, computed maps, and three-or-more-use functions. `dominant_container_tiny_helpers` emits one grouped review finding when a file has a configured large function/class plus enough non-exported 1–2-use ordinary helpers at or below the tiny-helper line limit. `--loose-low-value` broadens both rules to ordinary small bodies and component-local TSX functions, but still excludes React components, hooks, class methods, exports, tests by default, dead code, and functions with more than two references.

Verbose function output includes `role` (`reactComponent`, `reactHook`, `componentLocal`, `classMethod`, or `helper`), based on JSX, naming, ownership, and function kind, plus each function's dotted parent, sorted captures, and optional `lowValueReason`. Compact function tuples are `[file,line,col,name,exported,kind,parent,captures,lowValueReason]`; violations are `[file,rule,count,details]`.

### `scanr dupes`

Find structurally similar functions, interfaces, aliases, and type literals:

```bash
scanr dupes --root src
scanr dupes --root apps/web --types
scanr dupes --threshold 0.9 --min-lines 5 --print
```

Output is deterministic JSON: `{functions:[...],types:[...]}`. Functions are checked by default; `--types` also enables unified type comparison. `--print` adds source text to each location. Defaults are `--threshold 0.87` and `--min-lines 3`.

Place `// similarity-ignore` (or a block comment containing `similarity-ignore`) immediately above a function or type to exclude it. There is currently no name-pattern exclusion flag.

### `scanr slop`

Report deterministic, evidence-backed review signals:

```bash
scanr slop --root .
scanr slop --root . --base HEAD~1
scanr slop --root . --confidence high --json
scanr slop --root . --only scope-inflation,introduced-reinvention
scanr slop --root . --exclude generic-name-cluster --top 20
```

Markdown is the default and always uses one finding per row with full source line ranges and plain-English evidence, explanations, and actions. `--json` emits compact schema version 1 with a canonical absolute `root`, relative finding/evidence paths, and an optional diff summary. Output and filtering order are deterministic. Tables produced directly by `scanr slop`, or by reformatting its JSON, contain only Scanr-generated facts and findings; any ranking, recommendation, or interpretation added outside those fields is human review commentary and should be labeled separately.

These findings are review signals backed by offline syntax, resolution, reference, similarity, and test-shape evidence. Confidence is not AI-authorship attribution or a probability. **High** means the analyzer can prove the reported local condition from exact facts. **Medium** means the shape is evidenced but its design intent or semantic consequence still needs human review. `--confidence high` keeps High only; `--confidence medium` is a minimum threshold and keeps both Medium and High.

Flags:

- `--root <path>` — analysis root (default `.`)
- `--base <ref>` — opt into the three diff-only kinds; the ref is resolved to a commit before diffing
- `--confidence high|medium` — minimum confidence (default `medium`)
- `--only <kinds>` — retain only exact kebab-case names; comma-separated and repeatable
- `--exclude <kinds>` — remove names after `--only`; exclusion wins on overlap
- `--top <n>` — truncate after deterministic sorting; `0` is valid
- `--include-test-files` — include tests in ordinary contextual/low-value analysis; test-theater detectors still inspect their required test subjects
- `--json` — emit JSON schema version 1 instead of Markdown

`--only`, `--exclude`, `--confidence`, and `--top` apply after enabled analysis completes. Unknown kind names fail before filesystem scanning. With `--base`, Git compares the resolved commit with tracked staged and unstaged working-tree content. Untracked files are excluded. The three diff-only kinds activate only with `--base`; every non-diff detector continues to analyze the whole root.

Canonical kinds:

| Kind | Confidence | Signal |
| --- | --- | --- |
| `suppression-chain` | High | Multiple or nested static-check suppressions form one bypass chain. |
| `swallowed-failure` | High | An exact catch/rejection handler is empty or only logs before falling through. |
| `unresolved-api` | High | A local module, export, or namespace member is proven absent from a complete resolved surface. |
| `async-misuse` | High | A statically resolved async result is discarded. |
| `dead-surface` | High | A literal condition plus placeholder evidence proves a branch cannot execute. |
| `non-executing-test` | High | A recognized test registration is skipped, todo, or disabled. |
| `reinvented-helper` | Medium | A local helper structurally repeats an established cross-file helper. |
| `low-value-local-helper` | Medium | A balanced-rule top-level trivial helper has only one or two same-file references. |
| `dominant-container-tiny-helpers` | Medium | A 300+ line function/class sits beside at least two tiny low-use helpers. |
| `satellite-cluster` | Medium | One file contains at least two non-exported helpers of at most eight lines with one or two local references; non-trivial helpers must be single-use. |
| `redundant-defense` | Medium | A guard rechecks a value already proven non-null in the same function. |
| `one-use-abstraction` | Medium | A pass-through wrapper or interface has one demonstrated consumer/implementer. |
| `patch-stack` | Medium | One value receives at least three transforms across multiple categories. |
| `speculative-model` | Medium | A mostly optional model has little demonstrated cross-file field use. |
| `comment-inversion` | Medium | Trivial statements are narrated while the file's most complex function is unexplained. |
| `parallel-representation` | Medium | Near-identical models drift in optionality or resolved primitive type. |
| `generic-name-cluster` | Medium | Several generic declaration names include multiple load-bearing functions. |
| `assertion-monoculture` | Medium | A suite overwhelmingly repeats one recognized assertion shape without boundary/error coverage. |
| `mock-dominated-test` | Medium | Recognized mock operations dominate SUT calls and assertions across a suite. |
| `duplicated-test-body` | Medium | Four or more normalized test bodies differ only by literal vectors. |
| `implementation-mirroring-test` | Medium | Tests recompute a resolved production return expression. |
| `scope-inflation` | Medium, diff-only | A small runtime edit accompanies broad new files/helpers or a new dependency. |
| `introduced-reinvention` | Medium, diff-only | A fully added helper resembles a referenced helper outside the diff. |
| `generated-surface-burst` | Medium, diff-only | A diff adds at least three exports with no resolved analyzed-project references or four literal-only duplicate tests. |

Limitations: generated files and declaration files are excluded using path/header conventions; tests and package entrypoints receive detector-specific exclusions; dynamic imports, reflective/string-based use, unresolved re-exports, and incomplete parses limit absence claims; analyzed-project reference counts cannot prove that public exports have no external package, plugin, or generated consumers; option/config-property burst detection stays disabled without exact type-resolved ownership; structural similarity does not prove semantic interchangeability; diff scope excludes untracked files and cannot infer the user's requested scope. The tool remains offline and makes no AI-authorship, precision/recall, or universal quality claim.

### `scanr tree`

Print a compact project structure:

```bash
scanr tree
scanr tree --path src/commands
scanr tree --depth 4 --inline 8
scanr tree --all
scanr tree --functions
scanr tree --functions --all-functions
scanr tree --functions --function-details --function-min-lines 3 --function-max-lines 10
scanr tree --health --only-findings --top 20 --sort-by coupling
scanr tree --health --json
```

`--functions` annotates files with dotted function names. Anonymous callbacks are summarized as `+N anonymous`; `--all-functions` expands them. `--low-value-max-lines` controls `[L]` candidates, and `--duplicate-threshold` controls `[D:n]` groups (default `0.87`). Duplicate markers include one-line functions, and exact short matches are not size-penalized.

`--function-details` replaces compact markers with a nested, plain-English function tree. It shows parent components/functions even when they fall outside the selected `--function-min-lines` and `--function-max-lines` range, then annotates matching children with line counts, parent-variable usage, hoisting guidance, trivial-wrapper explanations, and similarity links.

Markers:

- `[H]` — the nested function uses no parent variables, so it can move outside its parent
- `[C:n]` — the function uses `n` variables from its parent and usually needs to remain nested
- `[L]` — the named function is a small trivial wrapper; review whether the abstraction adds value
- `[D:n]` — the function belongs to a similarity group containing `n` functions; use `dupes` for pair details

These markers are findings, not automatic refactoring instructions. Short React event handlers may be intentionally colocated even when marked `[L]`.

`--health` produces a deterministic file-level health report with file and largest-function size; named, nested, and anonymous function counts; parent-variable coupling; trivial-wrapper density; similarity groups and estimated removable lines; maximum branch complexity and control nesting; React hook/state/effect calls; exports; and imported dependency fan-out.

Health controls:

- `--only-findings` — omit files below every documented threshold
- `--top <n>` — limit results after deterministic sorting
- `--sort-by severity|coupling|duplicates|size` — rank hotspots (default `severity`)
- `--json` — emit `{version,root,files}` with metrics and structured findings

Medium/high thresholds are respectively: file lines `300/500`, total functions `15/30`, anonymous callbacks `10/20`, total parent-variable uses `15/30`, duplicate removable lines `10/30`, branch complexity `8/15`, control nesting `3/5`, hooks `8/15`, state calls `4/8`, effect calls `3/5`, dependencies `15/25`, and exports `8/15`. Wrapper density is reported at three or more wrappers and 30% (medium) or 50% (high).

## Agent workflow

1. `scanr slop --root .` — review exact, contextual, and test-theater signals one finding per row.
2. `scanr slop --root . --base HEAD~1` — add the three tracked-working-tree diff signals.
3. `scanr tree --health --only-findings --top 20` — rank metric-backed hotspots.
4. `scanr tree --functions --function-details --path <hotspot>` — inspect ownership and similarity in context.
5. `scanr scan --mode verbose --rules hoistable_nested_function,low_value_function` — inspect captures and rule evidence.
6. `scanr dupes --types` — inspect similar function/type pairs.
7. `scanr search <name> --json` — locate uses before refactoring.

## Development

```bash
cargo test
cargo build --release
```

## License

MIT. The vendored similarity engine under `src/similarity/` is from mizchi's `similarity-core` 0.5.2 under the MIT license; see `src/similarity/LICENSE`.
