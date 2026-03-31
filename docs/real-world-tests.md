# Real-World Test Checklist

Tested against: `igerslike-final/feat/ui-polish` (1020 TS/JS files, 176 folders, monorepo with apps/server + apps/web + packages/*)

## CLI Basics

- [x] `scanr --version` — prints `scanr 0.1.0`
- [x] `scanr --help` — lists all 8 commands: setup, index, search, tree, status, clear, reindex, scan
- [x] `scanr scan --help` — shows all flags with defaults and possible values
- [x] `scanr index --help` — shows `--max-bytes` flag with 512 KB default

## Tree Command

- [x] `scanr tree --root <path>` — default depth 6, shows full project structure
- [x] `scanr tree --depth 3` — collapses deep subtrees with `(76d 408f)` summaries
- [x] `scanr tree --path apps/web/src --depth 2` — focuses on a subdirectory
- [x] `scanr tree --all` — includes test directories (test/, tests/, __tests__)
- [x] Single-child directory chains collapse: `apps/server/src/api/routes/` on one line
- [x] Files with known code extensions have extensions stripped: `api.entry` not `api.entry.ts`
- [x] Non-code files keep extensions: `package.json`, `tsconfig.json`
- [x] Ignored directories skipped: node_modules, dist, build, target, .git, .next, .turbo, coverage
- [x] Hidden directories skipped: `.claude/`, `.glooit/`
- [x] Token count footer: `# ~356 tokens (1423 chars, 55 lines)`

## Scan Command — Output Modes

- [x] `--mode compact` — flat JSON: `{ver, stats, f:[], b:[], x:[]}` — 1020 files, 9219 functions, 24146 bindings, 2239 exports, 0 errors
- [x] `--mode verbose` — pretty JSON with spans, async/generator flags, export metadata
- [x] `--mode files` — functions grouped by file path with dot-notation nesting (`createActions.add`)
- [x] `--mode folders` — functions grouped by parent directory with counts (176 folders)

## Scan Command — Filters

- [x] `--include tsx` — 360 files, 2440 functions (TSX only)
- [x] `--include ts,tsx` — 1018 files, 9206 functions (TS + TSX)
- [x] `--exclude apps` — 194 files (packages only)
- [x] `--function-kinds top` — 2496 functions (declarations only)
- [x] `--function-kinds top+arrow` — 9108 functions (declarations + arrows)
- [x] `--function-kinds top+arrow+class` — 9133 functions (+ class methods, getters, setters, constructors)
- [x] `--function-kinds all` — 9219 functions (includes object methods, expressions)
- [x] `--max-bytes 500` — 102 files (skips files > 500 bytes)

## Scan Command — Single File

- [x] `--file <path> --mode verbose` — scans one file, returns functions with spans, exports, bindings
- [x] Relative path in output (`apps/web/src/application/router.tsx`, not absolute)
- [x] Detects arrow functions, declarations, async, generators, export status

## Scan Command — Correctness

- [x] 0 parse errors across 1020 files
- [x] Dot-notation nesting correct: `createActions.add`, `createActions.balance`, `main.shutdown`
- [x] Named exports detected: `makeAuthRoutes`, `makePaymentWebhookRoutes`
- [x] Default exports detected
- [x] Class methods, getters, setters, constructors extracted
- [x] Object methods extracted
- [x] Binding kinds: const, let, var, import, class, enum, catch, function
- [x] `file_indices` sorted by path

## Index Command

- [x] `.env` resolution from `--root` project directory (not just cwd)
- [x] File discovery: 1141 indexable files via `git ls-files`
- [x] `--max-bytes` skip: 1140 files chunked (1 file skipped: 1.9MB seed JSON > 512 KB limit)
- [x] Parallel chunking: 4972 chunks from 1140 files (no panics)
- [x] UTF-8 safety: handles multibyte characters (em-dash, arrow, emojis) without panicking
- [x] Incremental: skips unchanged files via SHA-256 hash comparison (1140 skipped on re-run)
- [x] Progress bar during chunking
- [x] Chunk truncation safety net: chunks > 12K chars truncated before embedding
- [x] Embedding + storage: 4972 chunks embedded and stored successfully
- [x] `--file <path>` single file re-index: `+0 added =0 skipped ~1 updated -6 deleted`
- [x] `--force` flag — re-embedded all 1140 files: `~1140 updated -4972 deleted`
- [x] `--gitignore <path>` override — applies as additional excludes via `core.excludesFile` (git design: affects untracked files)
- [x] `--chunk-size 1500 --chunk-overlap 200` — fewer chunks (4039 vs 4975) as expected with larger chunk size

## Search Command

- [x] `scanr search "authentication middleware"` — returns relevant auth middleware code, 10 results
- [x] `--json` output — structured JSON: `[{file, language, score, content}]`
- [x] `--files-only` — unique file paths with scores, no snippets
- [x] `--lang ts` — filters to typescript files only (maps `ts` -> `typescript` internally)
- [x] `--threshold 0.5` — returns only 2 results above 50% similarity
- [x] `--limit 2` — returns exactly 2 results
- [x] Stale file warning — `⚠ 27 file(s) changed since last index — run index to update`

## Status Command

- [x] Shows file count, chunk count, stale count
- [x] Lists stale files (up to 20 + "and N more")
- [x] Shows `run index to update` warning when stale > 0
- [x] After `clear`: shows 0 files, 0 chunks, all files as stale
- [x] After index: shows 1140 files, 4972 chunks

## Clear Command

- [x] `scanr clear --root <path>` — removes all indexed data, reports count

## Reindex Command

- [x] `scanr reindex --root <path>` — clears 4972 chunks then re-indexes: `+1140 added =1 skipped ~0 updated -0 deleted`

## Setup Command

- [ ] `scanr setup -y` — not re-tested (PostgreSQL already installed)
- [ ] `scanr setup --pg-version 18` — not re-tested
- [ ] Interactive version prompt — not re-tested

## Bugs Found and Fixed

1. **UTF-8 panic in chunking** (fixed) — `chunk.rs` sliced strings at byte boundaries instead of char boundaries when computing overlap. Files with multibyte characters (em-dash, arrow, emojis) caused panics. Fixed by adding `ceil_char_boundary()` helper.

2. **Tokio runtime panic in rayon threads** (fixed) — `index.rs` called `tokio::runtime::Handle::current()` inside rayon parallel iterators, which panics because rayon threads don't have access to the tokio runtime. Fixed by capturing the handle before entering the parallel section.

3. **`.env` resolution from cwd, not `--root`** (fixed) — `resolve_api_key()` only walked up from `cwd`, so running `scanr index --root /other/project` wouldn't find the API key in `/other/project/.env`. Fixed by also searching from the project root.

4. **Embedding token limit exceeded** (fixed) — Large files (e.g., 1.9MB seed JSON) produced chunks that exceeded OpenAI's `text-embedding-3-large` 8192 token limit. Fixed with two layers:
   - **`--max-bytes` flag** on the `index` command (default: 512 KB) skips oversized files during indexing
   - **Chunk truncation** safety net in `embed.rs` — any chunk exceeding 12,000 chars is truncated before sending to the API

5. **`--lang` filter didn't match stored language** (fixed) — `--lang ts` passed the raw extension to the DB query, but chunks store the full language name (`typescript`). Fixed by mapping the extension to the full language name via `git::lang_for_ext()` before querying.

## Test Environment

- **Platform:** macOS (Apple Silicon)
- **Rust:** 1.85+
- **PostgreSQL:** 18 with pgvector extension
- **Target codebase:** igerslike monorepo — 1020 TS/JS files, 176 folders, apps/server + apps/web + packages/*
