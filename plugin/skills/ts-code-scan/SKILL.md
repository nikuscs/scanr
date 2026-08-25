---
name: ts-code-scan
description: "constants, types, functions, tree, slop, refs, rename, census, dupes, and search for offline TypeScript and JavaScript"
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

## Rename a symbol

```bash
scanr rename <Name> <NewName> --root <path>
scanr rename src/card.tsx#Name <NewName> --root <path>   # when the bare name is ambiguous
```

Type-accurate via the TypeScript language service: imports, exports, aliases (public names preserved), barrels, JSX, interface/enum members, dynamic imports. Requires `bun` or `node` on PATH and a `tsconfig.json` (`--tsconfig <path>` when not at the root). `--dry-run` prints planned `files` and writes nothing.

JSON `leftovers` lists old-name occurrences the checker cannot rewrite — strings, comments, files outside the tsconfig. Review each; do not sed them blindly. After a write rename, run the project's typecheck (`tsc --noEmit`) to verify. Ambiguous bare names exit non-zero listing `file#Name` candidates — rerun with one.

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
