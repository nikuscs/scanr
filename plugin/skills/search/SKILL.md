---
name: search
description: Offline code search, structural TS/JS analysis, duplicate detection, and project tree overview.
argument-hint: [search pattern]
allowed-tools: Bash, Read
---

## Quick action

If `$ARGUMENTS` is provided, search immediately:

```bash
scanr search "$ARGUMENTS" --json
```

## Commands

### Content and path search

```bash
scanr search "useState" --json
scanr search "todo" --ignore-case --glob '*.ts' --context 2
scanr search --path "components" --json
```

### Tree overview

```bash
scanr tree
scanr tree --path src/commands
scanr tree --depth 4
scanr tree --functions              # dotted functions + H/C/L/D markers
scanr tree --functions --all-functions
```

### Structural scan

```bash
scanr scan --mode files
scanr scan --mode folders
scanr scan --file src/api.ts
scanr scan --mode compact
scanr scan --rules hoistable_nested_function,low_value_function
```

### Similarity detection

```bash
scanr dupes --root src
scanr dupes --root src --types
scanr dupes --threshold 0.9 --min-lines 5 --print
```

## Command selection

- Find a literal name or text → `scanr search`
- Find a file by path → `scanr search --path`
- Inspect repository shape → `scanr tree`
- Spot hoistable/capturing/low-value/duplicate functions → `scanr tree --functions`
- List functions, bindings, exports, captures, or violations → `scanr scan`
- Find similar functions or types → `scanr dupes`

## Flag reference

**search**: `--root <path>` `--path` `-i|--ignore-case` `--glob <pattern>` `--max-count N` `--context N` `--json`

**tree**: `--root <path>` `--path <subdir>` `--depth N` `--inline N` `--all` `--functions` `--all-functions` `--low-value-max-lines N`

Markers: `[H]` hoistable, `[C:n]` captures, `[L]` low-value candidate, `[D:n]` exact structural group.

**scan**: `--root <path>` `--mode compact|verbose|files|folders` `--file <path>` `--include ts,tsx,...` `--exclude <patterns>` `--function-kinds top|top+arrow|top+arrow+class|all` `--rules <rules>` `--include-test-files` `--low-value-max-lines N` `--max-bytes N`

**dupes**: `--root <path>` `--threshold <0-1>` `--min-lines N` `--types` `--print`
