---
name: ts-code-scan
description: "constants, types, functions, tree, slop, refs, census, dupes, and search for offline TypeScript and JavaScript"
allowed-tools: Bash, Read
---

If `scanr` is missing: `brew install nikuscs/tap/scanr`. Presenting findings to a human → [REPORTING.md](REPORTING.md).

For any folder, run **one** inventory (not verbose, not compact, not `head`):

```bash
scanr scan --mode inventory --root <path>
```

Done when `stats.parsed == stats.files` and `stats.errors == 0`. List nonzero `skipped`/`errors`. Then answer from the JSON keys below. Add `--lines` only if the user wants line numbers.

## Get all constants

Use `constants`. Each row is `{file, name, kind}` where `kind` is `primitive` or `arrow` (`const Foo = () => {}` is `arrow`).

Counts: `constants.length`, split by `kind`. A “quick list” is the `name`s, or `UPPER_SNAKE` names if they want real constants not hook locals.

## Get all types

Use `types`. Each row is `{file, name, kind}` where `kind` is `interface` or `type`.

## Get all functions / components / hooks / classes / enums / exports

Same inventory JSON:

- `functions` — named functions (dotted if nested)
- `components` — React components (`.tsx` + JSX + Uppercase)
- `hooks` — declarations named `use` + uppercase, not `useState` *calls*
- `classes`
- `enums`
- `exports`

## Get a tree

```bash
scanr tree --root <path>
```

`--path <subdir>` to focus. `--functions` for function markers. `--function-details` when they want nested components/functions explained. `--health --only-findings --top 20` for hotspots. Return the command output as printed.

## Usages of a name

```bash
scanr refs <name> --root <path>
```

Name-level oxc + import remapping, not tsserver. Usages exclude the declaration line.

## After names are known

Compact or verbose `scanr scan` for lines, captures, export flags, violations. `scanr scan --schema` for compact field layout. `scanr search "<name>" --json` for a literal hunt. `scanr dupes` for similar functions/types. `scanr slop` for slop evidence (`--base <ref>` only for tracked diff kinds).

## Refactor census

1. Inventory for names.
2. Compact scan for `exported` / positions (`f`, `t`, `x`). Compact `b` has no exported flag — join to `x` by name.
3. Done when `stats.parsed == stats.files` and `stats.errors == 0`.

## Slop kinds

`suppression-chain`, `swallowed-failure`, `unresolved-api`, `async-misuse`, `dead-surface`, `non-executing-test`, `reinvented-helper`, `low-value-local-helper`, `dominant-container-tiny-helpers`, `redundant-defense`, `one-use-abstraction`, `patch-stack`, `speculative-model`, `comment-inversion`, `parallel-representation`, `generic-name-cluster`, `assertion-monoculture`, `mock-dominated-test`, `duplicated-test-body`, `implementation-mirroring-test`, `scope-inflation`, `introduced-reinvention`, `generated-surface-burst`.

The first six are High. The rest are Medium. The last three are diff-only and need `--base`.

`--confidence` is a minimum. `--exclude` wins over `--only`. Dead-surface wording: no analyzed-project references were resolved.
