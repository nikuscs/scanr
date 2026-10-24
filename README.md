# 📡 scanr

[![Release](https://img.shields.io/github/v/release/nikuscs/scanr)](https://github.com/nikuscs/scanr/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A fast, deterministic static-analysis and search CLI for TypeScript and JavaScript projects. `scanr` is a single local binary: it needs no database, network access, service, or API key.

## Features

- **Structural analysis** — extract functions, captures, bindings, references, exports, and rule violations with oxc
- **Name inventory and refs** — folder-level name lists, then declaration usage sites
- **Type-accurate rename** — rewrite a symbol and its TypeScript-resolved usages (requires bun or node)
- **Content and path search** — gitignore-aware, parallel grep with stable JSON output
- **Similarity detection** — APTED-based function and type comparison, including type literals
- **Slop review signals** — evidence-backed exact, contextual, test-theater, and opt-in diff findings
- **Dead factory members** — unused members of factory-returned service objects, with an opt-in TypeScript-checked mode
- **Project tree** — compact repository structure for quick orientation
- **Project xray** — one-page overview of shape, hotspots, slop, and duplicates
- **Agent-friendly** — deterministic output suitable for scripts and coding agents

## Install

```bash
brew install nikuscs/tap/scanr
```

Or with Cargo:

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
scanr scan --root . --mode inventory
scanr refs Card --root .
scanr rename Card ProfileCard --root .
scanr dupes --root . --types
scanr slop --root .
scanr tree --root .
scanr xray --root .
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

Extract TypeScript and JavaScript structure. For agents, start with a name inventory, then use compact/verbose when you need lines, captures, or rules:

```bash
scanr scan --mode inventory
scanr scan --mode inventory --lines
scanr scan
scanr scan --root apps/web --mode verbose
scanr scan --file src/index.ts
scanr scan --mode files
scanr scan --mode folders
scanr scan --rules hoistable_nested_function
```

`--mode inventory` emits `{functions,constants,types,components,hooks,classes,enums,exports}` as `{file,name}` rows (add `--lines` for `line`). Constant `kind` is `primitive` or `arrow` (`const Foo = () => {}` is `arrow` and also appears under `functions`). Types are `interface` or `type`. `components` / `hooks` are the React role subset of functions (`.tsx` + JSX + Uppercase for components; `useX` for hooks).

Flags:

- `--root <path>` — project root
- `--mode compact|verbose|files|folders|inventory` — output shape
- `--lines` — include source line numbers in `--mode inventory`
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

### `scanr refs`

Find declarations and analyzed-project usage sites for a name (oxc symbols plus import remapping, not tsserver-grade find-all-references):

```bash
scanr refs Card
scanr refs MAX --root apps/web
```

JSON is `{ver,name,matches:[{declaration:{file,line,kind,exported},usages:[{file,line}]}]}`. Usages exclude the declaration line. This is name-level backtrace, not a language server.

### `scanr rename`

Rename a symbol and every type-resolved usage via the TypeScript language service (ts-morph). Requires `bun` or `node` on PATH, and a `tsconfig.json` (pass `--tsconfig` when it is not `<root>/tsconfig.json`).

```bash
scanr rename Card ProfileCard
scanr rename src/card.tsx#Card ProfileCard
scanr rename Card ProfileCard --dry-run
```

JSON is `{ver,renamed:{name,new_name,file,line},dry_run,files,leftovers:[{file,line,text}]}`. `files` lists rewritten paths; `leftovers` are remaining string/comment/non-renamed occurrences of the old name. `--dry-run` prints the same `files` list and writes nothing. An ambiguous bare name exits non-zero and lists every candidate as `path/to/file.tsx#Name`.

Flags:

- `--root <path>` — project root (default `.`)
- `--tsconfig <path>` — path to tsconfig.json (default `<root>/tsconfig.json`)
- `--dry-run` — report planned changes without writing files

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
scanr slop --root . --only passThroughModule,oversplitModule --oversplit-allow-importer '*.service.ts'
scanr slop --root . --only unusedFactoryMember --member-factory 'make*Service'
scanr slop --root . --type-aware --tsconfig apps/server/tsconfig.json --tsconfig apps/web/tsconfig.json
```

Markdown is the default and always uses one finding per row with full source line ranges and plain-English evidence, explanations, and actions. `--json` emits compact schema version 1 with a canonical absolute `root`, relative finding/evidence paths, and an optional diff summary. Output and filtering order are deterministic. Tables produced directly by `scanr slop`, or by reformatting its JSON, contain only Scanr-generated facts and findings; any ranking, recommendation, or interpretation added outside those fields is human review commentary and should be labeled separately.

These findings are review signals backed by offline syntax, resolution, reference, similarity, and test-shape evidence. Confidence is not AI-authorship attribution or a probability. **High** means the analyzer can prove the reported local condition from exact facts. **Medium** means the shape is evidenced but its design intent or semantic consequence still needs human review. `--confidence high` keeps High only; `--confidence medium` is a minimum threshold and keeps both Medium and High.

Flags:

- `--root <path>` — analysis root (default `.`)
- `--base <ref>` — opt into the three diff-only kinds; the ref is resolved to a commit before diffing
- `--confidence high|medium` — minimum confidence (default `medium`)
- `--only <kinds>` — retain canonical kebab-case names; comma-separated and repeatable. The new module kinds also accept `passThroughModule` and `oversplitModule`.
- `--exclude <kinds>` — remove names after `--only`; exclusion wins on overlap
- `--top <n>` — truncate after deterministic sorting; `0` is valid
- `--include-test-files` — include tests in ordinary contextual/low-value analysis; test-theater detectors still inspect their required test subjects
- `--oversplit-max-lines <n>` — maximum physical file lines for `oversplit-module` (default `30`, inclusive; comments and blank lines count)
- `--oversplit-allow-importer <glob>` — exempt oversplit findings whose single importer matches this gitignore-style path pattern; repeatable, e.g. `'*.service.ts'`
- `--member-factory <glob>` — exported factory names whose returned members are checked; `*` and `?` wildcards; repeatable (default `make*`)
- `--type-aware` — resolve factory members with the TypeScript checker; requires `bun` or `node` on PATH
- `--tsconfig <path>` — tsconfig for `--type-aware`; repeatable for monorepos (default: every `tsconfig.json` under the root outside `node_modules`)
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
| `pass-through-module` | Medium | A module only re-exports imports or exposes functions/objects forwarding unchanged arguments, without local logic. JSON kind: `passThroughModule`. |
| `oversplit-module` | Medium | A module of at most 30 physical lines has exactly one analyzed non-test importer, named in evidence. JSON kind: `oversplitModule`. |
| `unused-factory-member` | Medium | A member of the object an exported factory returns is never read. JSON kind: `unusedFactoryMember`. |
| `test-only-factory-member` | Medium, `--type-aware` | Only test files read the member. JSON kind: `testOnlyFactoryMember`. |
| `exposure-only-member` | Medium, `--type-aware` | Nothing reads the member, but production code reaches the function it wraps through other wiring. JSON kind: `exposureOnlyMember`. |
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

Module findings always skip tests, declaration/generated files, incomplete parses, and known package entrypoints, even with `--include-test-files`. Oversplit counts distinct non-test, non-generated importing files, including type/namespace imports, re-exports, and literal dynamic imports, but not side-effect-only imports. Generated-file detection includes an `automatically generated` marker in the first ten lines. Repeat `--oversplit-allow-importer` to combine exemptions, for example `--oversplit-allow-importer '*.service.ts' --oversplit-allow-importer 'rpc.router.ts' --oversplit-allow-importer 'vite.config.ts'`. It is a review signal, not proof that a file should be merged: public APIs, domain boundaries, and framework conventions can justify a small file. Import counts use the existing offline module resolver; unresolved aliases, non-literal dynamic imports, and external consumers are not counted. Pass-through proof is conservative: transformed/default arguments, side effects, async wrappers, and unsupported syntax are not reported.

Factory members come from the single top-level `return { ... }` (or arrow expression body) of an exported function matching `--member-factory`, including nested object literals as dotted paths such as `actions.profile.remember`. Only callable members and the objects that contain them are checked, because plain data may be serialized or read by reflection. A dead object reports its topmost dead path; a dead object with one child reports that child.

- **Syntactic (default)** — name-based and offline. A member is reported only when no analyzed file reads a property with that name (`a.b`, `a?.b`, `a['b']`, `{ b } = a`, `{ b: c }`, parameter destructuring, `<a.B />`, `typeof a.b`, `T['b']`) outside the member's own declaration. It misses members whose name is read anywhere else, by design. Factories with no member read at all, or whose call result is passed straight to another call (`f(make())`, `f({ key: make() })`), are skipped because a library may consume the whole object.
- **Type-aware (`--type-aware`)** — the bundled TypeScript worker builds one program per tsconfig and resolves every property access, element access, destructuring and indexed type to the returned members, across all programs. Test files use the same classification as the rest of `slop`. References in type positions (`Service['member']`, `typeof service.member`) are counted as type-only and do not keep a member alive. A member is `exposure-only` when it (or every forwarding member below it) wraps a function, value or local object that production code references outside the member. When a factory result flows into another declared type (an annotated return type, a typed parameter or property), reads through that type count for the member; when it flows into a library type (`streamText({ tools: makeTools() })`), the factory is skipped.

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

### `scanr xray`

Print a one-page project overview for an agent opening an unknown repository:

```bash
scanr xray
scanr xray --root apps/web --top 5
scanr xray --json
```

It combines data from the other commands into four sections: **Shape** (file, line, and function counts plus the largest folders), **Hotspots** (files over `tree --health` thresholds, ranked by severity), **Slop** (finding counts per kind plus the top findings, High confidence first), and **Duplicates** (similar-function groups ranked by estimated removable lines). Tests, declaration files, and generated files are excluded. Markdown is the default; `--json` emits `{version,root,shape,hotspots,slop,duplicates}`.

Flags:

- `--root <path>` — project root (default `.`)
- `--top <n>` — maximum rows per section (default `10`)
- `--json` — emit JSON instead of Markdown

Each section points to the command with full detail: `tree --health`, `slop`, and `dupes`.

## Agent workflow

1. `scanr xray --root .` — one-page overview before anything else.
2. `scanr scan --mode inventory --root .` — name lists first (functions, constants, types, components, hooks, classes, enums, exports).
3. `scanr refs <name> --root .` — declarations and usage sites for a name.
4. `scanr rename <Name|file#Name> <NewName> --root . --dry-run` — preview a type-accurate rename, then rerun without `--dry-run` and review `leftovers`.
5. `scanr slop --root .` — review exact, contextual, and test-theater signals one finding per row.
6. `scanr slop --root . --base HEAD~1` — add the three tracked-working-tree diff signals.
7. `scanr tree --health --only-findings --top 20` — rank metric-backed hotspots.
8. `scanr tree --functions --function-details --path <hotspot>` — inspect ownership and similarity in context.
9. `scanr scan --mode verbose --rules hoistable_nested_function,low_value_function` — inspect captures and rule evidence.
10. `scanr dupes --types` — inspect similar function/type pairs.
11. `scanr search <name> --json` — literal hunt when refs is too narrow.

## Development

```bash
cargo test
cargo build --release
make worker   # rebuild assets/worker.mjs after editing worker/*.ts (needs bun)
```

## License

MIT. The vendored similarity engine under `src/similarity/` is from mizchi's `similarity-core` 0.5.2 under the MIT license; see `src/similarity/LICENSE`.
