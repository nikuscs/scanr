---
name: ts-code-scan
description: "scanr CLI for offline TS/JS analysis: slop review signals, name inventory for functions/constants/types, declaration usage backtrace, export census for renames/refactors, duplicate and similar-function detection, structural scan, content search, tree overview."
argument-hint: [search pattern]
allowed-tools: Bash, Read
---

## Quick action

If `$ARGUMENTS` is provided, search immediately:

```bash
scanr search "$ARGUMENTS" --json
```

## Command selection

Pick one command, then `scanr <cmd> --help` for flags. Presenting findings to a human → [REPORTING.md](REPORTING.md).

| Ask | Command |
| --- | --- |
| Literal name or text | `scanr search` |
| File by path | `scanr search --path` |
| Repository shape | `scanr tree` |
| Name inventory of a folder | `scanr scan --mode inventory` |
| Usages of a declaration | `scanr refs <name>` |
| Hoistable, capturing, low-value, or duplicate functions | `scanr tree --functions` |
| Health hotspots (complexity, coupling, size, fan-out) | `scanr tree --health --only-findings --top 20` |
| Nested component/function tree with explanations | `scanr tree --functions --function-details --function-min-lines 3 --function-max-lines 10` |
| Functions, bindings, types, exports, captures, violations | `scanr scan` |
| Export census before a mass rename | `scanr scan --mode inventory` then compact — **Refactor census** |
| Similar functions or types | `scanr dupes` |
| Slop, reinvention, test-theater, or diff-inflation evidence | `scanr slop`; add `--base <ref>` only for tracked diff evidence |

Start with `--mode inventory`. Then `scanr refs <name>` for usages. Use compact/verbose/`--file` only after you know which names matter. Health metrics stay on `tree --health`. Slop detector evidence stays on `slop`.

## Name inventory

Default agent scan of a folder:

```bash
scanr scan --mode inventory --root <path>
scanr scan --mode inventory --root <path> --lines
```

JSON keys: `functions`, `constants`, `types`, `components`, `hooks`, `classes`, `enums`, `exports`. Rows are `{file,name}` (`kind` on constants and types). `--lines` adds `line`.

Constant `kind` is `primitive` or `arrow`. `const Foo = () => {}` is `arrow` and also listed under `functions`. Types are `interface` or `type`. `components` / `hooks` are the React role subset of functions.

The inventory is complete when `stats.parsed == stats.files` and `stats.errors == 0`. List every nonzero `skipped` or `errors` value.

## Refs

```bash
scanr refs Card --root <path>
```

JSON: `{ver,name,matches:[{declaration,usages}]}`. Name-level oxc + import remapping, not tsserver find-all-references. Usages exclude the declaration line.

## Refactor census

1. `scanr scan --mode inventory` for names.
2. `scanr scan --schema` when the compact field layout is unknown.
3. `scanr scan` (compact) for lines, export flags, captures.
4. The census is complete when `stats.parsed == stats.files` and `stats.errors == 0`. List every nonzero `skipped` or `errors` value, and the `err` messages.
5. Build the From→To table from inventory names, then compact `f` / `t` / `x` when you need `exported` or positions. Compact `b` has no exported flag — join to `x` by name.

## Commands

```bash
scanr search "useState" --json
scanr search "todo" --ignore-case --glob '*.ts' --context 2
scanr search --path "components" --json

scanr tree
scanr tree --path src/commands
scanr tree --functions
scanr tree --functions --function-details --function-min-lines 3 --function-max-lines 10
scanr tree --health --only-findings --top 20

scanr scan --mode inventory
scanr scan --mode inventory --lines
scanr refs Card
scanr scan
scanr scan --schema
scanr scan --mode verbose
scanr scan --file src/api.ts
scanr scan --rules hoistable_nested_function,low_value_function,low_value_local_helper

scanr dupes --root src
scanr dupes --root src --types

scanr slop --root .
scanr slop --root . --base HEAD~1
scanr slop --root . --confidence high --json
scanr slop --root . --only scope-inflation,introduced-reinvention
```

`--function-details` is the one tree when the user wants files, React components, nested functions, ownership, line counts, or cross-component similarities together. Return that tree as printed.

Treat `--health` severity as threshold evidence. State the concrete metrics behind any recommendation.

## Slop kinds

`suppression-chain`, `swallowed-failure`, `unresolved-api`, `async-misuse`, `dead-surface`, `non-executing-test`, `reinvented-helper`, `low-value-local-helper`, `dominant-container-tiny-helpers`, `redundant-defense`, `one-use-abstraction`, `patch-stack`, `speculative-model`, `comment-inversion`, `parallel-representation`, `generic-name-cluster`, `assertion-monoculture`, `mock-dominated-test`, `duplicated-test-body`, `implementation-mirroring-test`, `scope-inflation`, `introduced-reinvention`, `generated-surface-burst`.

The first six are High. The rest are Medium. The last three are diff-only and need `--base`.

`--confidence` is a minimum. `--exclude` wins over `--only`. Filters apply after analysis. Markdown is the default; JSON is schema version 1.

With `--base`, staged and unstaged tracked content is compared to the resolved commit, untracked files are excluded, and non-diff findings still cover the whole root. Dead-surface wording: no analyzed-project references were resolved.
