# Slop analysis foundation (Phases 1–3)

`scanr slop` reports deterministic review evidence. It does not identify AI authorship or estimate an AI probability. The Phase 1–3 command intentionally has an empty detector registry; later detector phases consume the neutral facts and project indexes described here.

## Deterministic offline coverage

The existing OXC parse produces owned, root-relative facts for comments/directives, catches, assertions/casts, calls/awaits, literal branches, imports, declarations, interfaces/type literals, member uses, recognized test registrations, function metrics, generated/test classification, and runtime lines. `ProjectFacts` sorts and deduplicates every index, resolves local extension/index imports, strict-JSON `tsconfig` path aliases, and installed package entry/type files, and distinguishes `Resolved`, `Missing`, `Ambiguous`, and `Unknown` results.

Malformed or recovered files set `analysisComplete: false`; project coverage records them so later absence-based detectors can stay silent. Dynamic imports, unresolved star re-exports, unsupported `tsconfig` syntax, and external package surfaces are recorded as incomplete rather than converted into missing-API claims.

## Unsupported offline proofs

`AnalysisCoverage.unsupportedOfflineProofs` explicitly records claims the foundation cannot prove without a TypeScript checker, whole-runtime knowledge, or task intent:

- that a catch default/recovery value swallowed a failure;
- that `async` without `await`, or sequential `await`, is misuse;
- that a member is absent after TypeScript augmentation/generic/union resolution;
- that an export, property, or manifest dependency has no dynamic/external consumer;
- that a custom assertion API executes a callback;
- that structurally similar helpers are semantically interchangeable;
- that a Git diff exceeds the user’s requested scope.

Later lanes must omit or downgrade findings that depend on these proofs. They must not replace missing proof with source regex, network/registry queries, or model inference.

## Phase boundary

`--base` is present in the CLI contract but is rejected with an explicit message until the diff-aware phase is implemented; no Git command runs in Phase 1–3. `--only` and `--exclude` validate all reserved detector names before scanning. With the empty registry, Markdown emits the stable table header and JSON emits schema version 1 with an empty `findings` array.
