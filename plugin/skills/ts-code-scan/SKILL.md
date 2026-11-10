---
name: ts-code-scan
description: "xray overview, file outline, role drift, dependencies, outline diff, constants, types, functions, tree, slop, refs, rename, census, dupes, and search for offline TypeScript and JavaScript"
allowed-tools: Bash, Read
---

If `scanr` is missing: `brew install nikuscs/tap/scanr`. Presenting findings to a human → [REPORTING.md](REPORTING.md).

For any folder, run **one** inventory (not verbose, not compact, not `head`):

```bash
scanr scan --mode inventory --root <path>
```

Done when `stats.parsed == stats.files` and `stats.errors == 0`. List nonzero `skipped`/`errors`. Then answer from the JSON keys below. Add `--lines` only if the user wants line numbers.

## Xray a project

When the user wants a quick overview of an unfamiliar repo:

```bash
scanr xray --root <path>
```

One Markdown page: shape, health hotspots, slop counts, and duplicate groups. `--top <n>` limits rows per section; `--json` for structured output. Return the output as printed, then drill in with `tree --health`, `slop`, or `dupes`.

## See how files are organized

When the question is what lives inside each file, how declarations nest, or whether a folder follows its structure:

```bash
scanr scan --mode outline --root <path>          # JSON for you
scanr tree --outline --root <path> --path <dir>  # tree for a human
```

Each file lists its `symbols` as a tree: `{name, kind, line, endLine}` plus `via` (the callee that produced it, such as `Effect.fn`), `extends`, `exported`, `exportedAs`, and `children`. `reexports` rows carry `from` and either `names` or `all`. A declaration inside an anonymous callback belongs to the nearest named declaration around it, so `record` sits under `make` in `const make = Effect.gen(function* () { const record = … })`.

A variable is a `function` only when assigned an arrow, a function expression, or a recognized wrapper (`Effect.fn`, `Effect.fnUntraced`, `memo`, `forwardRef`, `useCallback`, `cache`, `debounce`, `throttle`, `memoize`, `once`, `observer`). Any other call result is a `const` with `via`; read `via` to judge it.

Default output is declarations and functions. `--deep` adds locals, properties, type and enum members, and imports. `--signatures` adds each callable's parameters and return type. `--exported` keeps only exported symbols and re-exports, the public surface. In the tree, indentation means "lives inside" and `()` marks a function; return it as printed.

## Check a folder follows its architecture

```bash
scanr roles --root <path>
```

Groups files by name suffix (`*.layer.ts` is `.layer`) and lists, per role, what its files usually have and the files that differ: `missing` a usual trait, or carrying an `unusual` one such as `imports .fake (1 of 18)`. Read each odd one out as a question to check, not a verdict. `--min-files`, `--usual`, and `--unusual` tune the thresholds; `--json` for structured output.

## What a file depends on, and what depends on it

Before moving, splitting, or deleting a file:

```bash
scanr deps --root <path> --path <file-or-dir>
```

Lists resolved project imports (`(type)` when type-only), packages, unresolved relative imports, and `imported by`. Importers come from the whole root, tests included. `--json` for structured output.

## Review a refactor by its structure

```bash
scanr diff --base <ref> --root <path>
```

Compares the working tree's outlines with `<ref>`: `+` added, `-` removed, `>` moved (with the destination file), `~` renamed or changed (signature, exported, kind, producer). Renames are claimed only with evidence, so a rename it cannot prove shows as one removal and one addition. Run it after a refactor to confirm nothing was dropped or left behind. `--deep` includes locals; `--json` for structured output.

## Get all constants

Use `constants`. Each row is `{file, name, kind}` where `kind` is `primitive` or `arrow` (`const Foo = () => {}` is `arrow`). Names are dotted paths through the enclosing declarations (`make.db`), so a name without a dot is top-level. `let` and `var` declarations are under `variables`.

Counts: `constants.length`, split by `kind`. A “quick list” is the `name`s, or `UPPER_SNAKE` names if they want real constants not hook locals.

## Get all types

Use `types`. Each row is `{file, name, kind}` where `kind` is `interface` or `type`.

## Get all functions / components / hooks / classes / enums / namespaces / exports

Same inventory JSON:

- `functions` — named functions, methods included (dotted path: `make.record`, `Service.run`)
- `components` — React components (`.tsx` + JSX + Uppercase)
- `hooks` — declarations named `use` + uppercase, not `useState` *calls*
- `classes`
- `enums`
- `namespaces`
- `exports` — each public name once; re-exported names carry `from` (`*` for `export * from`)

## Get a tree

```bash
scanr tree --root <path>
```

`--path <subdir>` to focus. `--outline` for each file's nested declarations. `--functions` for function markers. `--function-details` when they want nested components/functions explained. `--health --only-findings --top 20` for hotspots. Return the command output as printed.

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

`suppression-chain`, `swallowed-failure`, `unresolved-api`, `async-misuse`, `dead-surface`, `non-executing-test`, `reinvented-helper`, `low-value-local-helper`, `dominant-container-tiny-helpers`, `redundant-defense`, `one-use-abstraction`, `patch-stack`, `speculative-model`, `comment-inversion`, `parallel-representation`, `generic-name-cluster`, `assertion-monoculture`, `mock-dominated-test`, `duplicated-test-body`, `implementation-mirroring-test`, `scope-inflation`, `introduced-reinvention`, `generated-surface-burst`, `pass-through-module`, `oversplit-module`, `unused-factory-member`, `test-only-factory-member`, `exposure-only-member`.

The first six are High. The rest are Medium. `scope-inflation`, `introduced-reinvention` and `generated-surface-burst` are diff-only and need `--base`. `test-only-factory-member` and `exposure-only-member` need `--type-aware`.

Dead members of factory-returned services (`makeRedisService` returning an unused `decrement`): `scanr slop --root <path> --type-aware --member-factory 'make*Service' --only unusedFactoryMember,testOnlyFactoryMember,exposureOnlyMember`. Add `--tsconfig <path>` once per app in a monorepo. Needs bun or node.

`--confidence` is a minimum. `--exclude` wins over `--only`. Dead-surface wording: no analyzed-project references were resolved.
