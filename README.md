# 📡 scanr

[![Release](https://img.shields.io/github/v/release/nikuscs/scanr)](https://github.com/nikuscs/scanr/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A fast, deterministic static-analysis and search CLI for TypeScript and JavaScript projects. `scanr` is a single local binary: it needs no database, network access, service, or API key.

## Features

- **Structural analysis** — extract functions, captures, bindings, references, exports, and rule violations with oxc
- **Content and path search** — gitignore-aware, parallel grep with stable JSON output
- **Similarity detection** — APTED-based function and type comparison, including type literals
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
- `--low-value-max-lines <n>` — maximum candidate length (default `3`)
- `--file <path>` — scan one file

Built-in rules are `no_unused_bindings`, `one_exported_function_per_file`, `max_functions_per_file`, `hoistable_nested_function`, and `low_value_function`. The low-value rule is deliberately narrow: it reports named functions within the line limit only when the AST is empty, a constant/identity/property return, or a thin call wrapper.

Function output includes each function's dotted parent, sorted captures, and optional `lowValueReason`. Compact function tuples are `[file,line,col,name,exported,kind,parent,captures,lowValueReason]`; violations are `[file,rule,count,details]`.

### `scanr dupes`

Find structurally similar functions, interfaces, aliases, and type literals:

```bash
scanr dupes --root src
scanr dupes --root apps/web --types
scanr dupes --threshold 0.9 --min-lines 5 --print
```

Output is deterministic JSON: `{functions:[...],types:[...]}`. Functions are checked by default; `--types` also enables unified type comparison. `--print` adds source text to each location. Defaults are `--threshold 0.87` and `--min-lines 3`.

Place `// similarity-ignore` (or a block comment containing `similarity-ignore`) immediately above a function or type to exclude it. There is currently no name-pattern exclusion flag.

### `scanr tree`

Print a compact project structure:

```bash
scanr tree
scanr tree --path src/commands
scanr tree --depth 4 --inline 8
scanr tree --all
scanr tree --functions
scanr tree --functions --all-functions
```

`--functions` annotates files with dotted function names. Anonymous callbacks are summarized as `+N anonymous`; `--all-functions` expands them. `--low-value-max-lines` controls `[L]` candidates, and `--duplicate-threshold` controls `[D:n]` groups (default `0.87`). Duplicate markers include one-line functions, and exact short matches are not size-penalized.

Markers:

- `[H]` — nested function captures nothing and can be hoisted
- `[C:n]` — captures `n` enclosing function bindings
- `[L]` — conservative low-value candidate
- `[D:n]` — function belongs to an `n`-member similarity group at the configured threshold; use `dupes` for pair details

## Agent workflow

1. `scanr tree --functions --depth 5` — orient and spot marked hotspots.
2. `scanr scan --mode verbose --rules hoistable_nested_function,low_value_function` — inspect captures and evidence.
3. `scanr dupes --types` — inspect similar function/type pairs.
4. `scanr search <name> --json` — locate uses before refactoring.

## Development

```bash
cargo test
cargo build --release
```

## License

MIT. The vendored similarity engine under `src/similarity/` is from mizchi's `similarity-core` 0.5.2 under the MIT license; see `src/similarity/LICENSE`.
