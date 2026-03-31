---
name: search
description: Semantic codebase search, structural TS/JS analysis, and project tree overview. Search by meaning using OpenAI embeddings + pgvector, extract functions/exports with oxc, or get a compact tree view.
argument-hint: [search query]
allowed-tools: Bash, Read
---

## Index state

!`scanr status 2>/dev/null || echo "Not indexed — run scanr setup -y && scanr index"`

`scanr status` shows whether the index is stale and which embedding config the project is using.

## Quick action

If `$ARGUMENTS` is provided, search immediately:

```bash
scanr search "$ARGUMENTS" --json --limit 10
```

If no arguments, check the index state above and decide based on the user's request.

## Commands

### Semantic search (requires setup + index)

```bash
scanr search "how does auth work" --json --limit 10
scanr search "payment processing" --files-only        # file paths only
scanr search "API routes" --lang ts --threshold 0.5    # filter by language
```

### Tree overview (no setup needed)

```bash
scanr tree                        # compact structure
scanr tree --path src/commands    # focus on subtree
scanr tree --depth 4              # shallower
```

### Structural scan — TS/JS only (no setup needed)

```bash
scanr scan --mode files           # functions grouped by file
scanr scan --mode folders         # functions grouped by folder
scanr scan --file src/api.ts      # single file
scanr scan --mode compact         # flat JSON arrays, smallest output
```

### Maintenance

```bash
scanr index                       # incremental re-index
scanr index --embedding openai:text-embedding-3-small
scanr index --file <path>         # re-index one file after editing
scanr reindex                     # full re-index from scratch
scanr reindex --embedding openai:text-embedding-3-small
scanr clear                       # remove all indexed data for this project
scanr setup -y                    # first-time setup (PostgreSQL + pgvector)
```

## How to pick the right command

- **"find where X is implemented"** / **"how does X work"** → `scanr search` (semantic)
- **"show me the project structure"** / **"what's in this repo"** → `scanr tree`
- **"list all functions"** / **"what does this file export"** → `scanr scan`
- **"find similar patterns"** / **"code that does X"** → `scanr search`

## Agent tips

- Always use `--json` for search — returns `[{file, language, score, content}]`
- Use `--files-only` for orientation before deep-reading with the Read tool
- Keep `--limit 5-10` to stay within context
- If the index state above shows stale files, run `scanr index` before searching
- Check the embedding line in `scanr status` if you need to verify which provider/model/dimensions the project is using
- `search` does not take an embedding flag; it always uses the project's stored embedding config
- After editing files, re-index: `scanr index --file <path>`
- Phrase queries as what the code *does*: "debit wallet integer amount" not "where is money subtracted"
- `scan` and `tree` work instantly — no database or API key needed
- `search` requires `OPENAI_API_KEY` in env, `~/.zshrc`, or a `.env` file (walks up to 6 dirs)
- `SCANR_EMBEDDING=openai:text-embedding-3-small` can set the default embedding for new indexes

## Flag reference

All commands accept `--root <path>` (default: `.`).

**search**: `--limit N` `--threshold 0-1` `--lang <ext>` `--files-only` `--json`

**index**: `--embedding provider:model` `--file <path>` `--force` `--chunk-size N` `--chunk-overlap N` `--gitignore <path>`

**tree**: `--path <subdir>` `--depth N` `--inline N` `--all`

**scan**: `--mode compact|verbose|files|folders` `--file <path>` `--include ts,tsx,...` `--exclude <dirs>` `--function-kinds top|top+arrow|top+arrow+class|all` `--rules <rules>` `--max-bytes N`

**setup**: `-y` `--pg-version N`
