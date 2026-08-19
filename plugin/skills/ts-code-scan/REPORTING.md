# Human scanr reports

Load this file when presenting scanr findings to a human. Agent-consumed JSON skips it.

## Slop

Return the command's one-finding-per-row Markdown table. Keep each row's confidence, evidence, line range, explanation, action, and relevant limitations. Confidence is evidence strength, not probability or AI attribution.

Dead-surface wording: no analyzed-project references were resolved.

## Function trees

Translate scanner markers:

| Marker | Wording |
| --- | --- |
| `[H]` | Can move outside its parent function because it uses no parent variables. |
| `[C:n]` | Uses `n` variables from its parent function. |
| `[L]` | Small trivial-wrapper candidate; review whether the named function adds value. |
| `[D:n]` | Belongs to a similarity group containing `n` functions. |

Line counts go under a **Lines** column as numbers.

| File | Component | Function | Lines | Why flagged | Suggested action |
| --- | --- | --- | ---: | --- | --- |
| `components/example.tsx` | `Example` | `handleSave` | 4 | Small wrapper using two parent variables | Keep if it clarifies the event; otherwise inline |

One finding per row. Relative paths. Separate Component and Function columns. Sort by file path, then source line, unless the user asked for severity ranking. A short event handler may be intentional — flag it, and leave the keep-or-inline call to the user.
