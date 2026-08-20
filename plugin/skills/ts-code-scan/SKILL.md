---
name: ts-code-scan
description: "slop, inventory of constants/functions/types, refs, census, dupes, search, and tree for offline TypeScript and JavaScript"
allowed-tools: Bash, Read
---

## Command selection

If `scanr` is missing: `brew install nikuscs/tap/scanr`. Pick one command, then `scanr <cmd> --help` for flags. Presenting findings to a human → [REPORTING.md](REPORTING.md).

| Ask | Command |
| --- | --- |
| Literal name or text | `scanr search` |
| File by path | `scanr search --path` |
| Repository shape | `scanr tree` |
| List constants, functions, types, components, hooks, classes, enums, or exports | `scanr scan --mode inventory` |
| Usages of a declaration | `scanr refs <name>` |
| Hoistable, capturing, low-value, or duplicate functions | `scanr tree --functions` |
| Health hotspots (complexity, coupling, size, fan-out) | `scanr tree --health --only-findings --top 20` |
| Nested component/function tree with explanations | `scanr tree --functions --function-details` |
| Lines, captures, export flags, or rule violations after inventory | `scanr scan` compact or verbose |
| Export census before a mass rename | `scanr scan --mode inventory` then compact — **Refactor census** |
| Similar functions or types | `scanr dupes` |
| Slop, reinvention, test-theater, or diff-inflation evidence | `scanr slop`; add `--base <ref>` only for tracked diff evidence |

Name lists and counts come from `--mode inventory`. Summarize the JSON keys; do not stream compact or verbose for a constants/functions/types list. Then `scanr refs <name>` for usages. Compact/verbose/`--file` only after you know which names matter. Health metrics stay on `tree --health`. Slop detector evidence stays on `slop`.

`--function-details` is the one tree when the user wants files, React components, nested functions, ownership, line counts, or cross-component similarities together. Return that tree as printed. Treat `--health` severity as threshold evidence and state the concrete metrics behind any recommendation.

## Name inventory

```bash
scanr scan --mode inventory --root <path>
```

Add `--lines` for source lines. Constant `kind` is `primitive` or `arrow`. `components` / `hooks` are the React role subset of functions.

The inventory is complete when `stats.parsed == stats.files` and `stats.errors == 0`. List every nonzero `skipped` or `errors` value.

## Refs

```bash
scanr refs Card --root <path>
```

Name-level oxc + import remapping, not tsserver find-all-references. Usages exclude the declaration line.

## Refactor census

1. `scanr scan --mode inventory` for names.
2. `scanr scan --schema` when the compact field layout is unknown.
3. `scanr scan` (compact) for lines, export flags, captures.
4. The census is complete when `stats.parsed == stats.files` and `stats.errors == 0`. List every nonzero `skipped` or `errors` value, and the `err` messages.
5. Build the From→To table from inventory names, then compact `f` / `t` / `x` when you need `exported` or positions. Compact `b` has no exported flag — join to `x` by name.

## Slop kinds

`suppression-chain`, `swallowed-failure`, `unresolved-api`, `async-misuse`, `dead-surface`, `non-executing-test`, `reinvented-helper`, `low-value-local-helper`, `dominant-container-tiny-helpers`, `redundant-defense`, `one-use-abstraction`, `patch-stack`, `speculative-model`, `comment-inversion`, `parallel-representation`, `generic-name-cluster`, `assertion-monoculture`, `mock-dominated-test`, `duplicated-test-body`, `implementation-mirroring-test`, `scope-inflation`, `introduced-reinvention`, `generated-surface-burst`.

The first six are High. The rest are Medium. The last three are diff-only and need `--base`.

`--confidence` is a minimum. `--exclude` wins over `--only`. Filters apply after analysis. Markdown is the default; JSON is schema version 1.

With `--base`, staged and unstaged tracked content is compared to the resolved commit, untracked files are excluded, and non-diff findings still cover the whole root. Dead-surface wording: no analyzed-project references were resolved.
