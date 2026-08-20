---
name: ts-code-scan
description: "scanr CLI for offline TS/JS analysis: slop review signals, export census for renames/refactors, duplicate and similar-function detection, structural scan, content search, tree overview."
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
| Hoistable, capturing, low-value, or duplicate functions | `scanr tree --functions` |
| Health hotspots (complexity, coupling, size, fan-out) | `scanr tree --health --only-findings --top 20` |
| Nested component/function tree with explanations | `scanr tree --functions --function-details --function-min-lines 3 --function-max-lines 10` |
| Functions, bindings, types, exports, captures, violations | `scanr scan` |
| Export census before a mass rename | `scanr scan` — **Refactor census** |
| Similar functions or types | `scanr dupes` |
| Slop, reinvention, test-theater, or diff-inflation evidence | `scanr slop`; add `--base <ref>` only for tracked diff evidence |

Health metrics stay on `tree --health`. Slop detector evidence stays on `slop`.

## Refactor census

Build a complete, verifiable export inventory before a mass rename.

1. `scanr scan --schema` when the compact field layout is unknown.
2. `scanr scan` (default compact).
3. The census is complete when `stats.parsed == stats.files` and `stats.errors == 0`. List every nonzero `skipped` or `errors` value, and the `err` messages.
4. Build the From→To table from `f` (functions), `t` (types: `interface` / `type`), and `x` (every export). `f` and `t` carry `exported`. `b` has no exported flag — join to `x` by name for binding export status.
5. `--mode files` lists names and drops export status; use compact or verbose for a rename inventory.

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
