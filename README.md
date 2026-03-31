# 📡 scanr

[![Release](https://img.shields.io/github/v/release/nikuscs/scanr)](https://github.com/nikuscs/scanr/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Semantic codebase search + TypeScript structural analysis. Works as a skill for [Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Claude.ai](https://claude.ai), [OpenAI Codex](https://openai.com/index/openai-codex/), and any AI agent.**

Two modes: **semantic search** (OpenAI embeddings + pgvector) for finding code by meaning, and **structural scan** (oxc parser) for extracting functions, bindings, and exports from TypeScript/JavaScript projects.

> **Note:** This project was built with AI assistance. Review, test, and verify before using in production.

### Features
 
- 🔍 **Semantic search** — find code by meaning, not keywords (embeddings + pgvector)
- 🧬 **TypeScript analysis** — extract functions, bindings & exports via oxc
- 🌳 **Tree view** — compact project structure for fast orientation
- ⚡ **Fast** — parallel chunking (rayon), HNSW index, batch embeddings
- 📦 **Incremental** — SHA-256 dedup, only re-embeds changed files
- 🔧 **Zero config** — auto-installs PostgreSQL + pgvector via Homebrew
- 🌐 **Multi-language** — tree-sitter for TS/JS/Rust/Python/Go + plain chunking for data files
- 🤖 **Agent-friendly** — JSON output, non-interactive setup, stale warnings to stderr

## What scanr is

- A **local CLI for codebase retrieval** that indexes the repository you already have on disk
- A **semantic code search tool** built on OpenAI embeddings + pgvector
- A **TypeScript/JavaScript structural scanner** powered by oxc, for extracting functions, bindings, and exports
- An **agent-friendly interface** with JSON output, non-interactive setup, and stale index warnings
- A **batteries-included retrieval pipeline**: file discovery, chunking, embedding, storage, and search in one tool

## What scanr is not

- Not a **vector database** or general-purpose retrieval backend like pgvector, Qdrant, Pinecone, Chroma, or turbopuffer
- Not an **IDE** or editor plugin
- Not a **hosted codebase Q&A service** or PR review product
- Not an **enterprise code intelligence platform** for organization-wide search across many repositories
- Not a replacement for **grep**, **language servers**, or full code navigation tools
- Not a fully offline tool today: semantic search depends on OpenAI embeddings, while `scan` and `tree` do not

## Install

```bash
# From source (requires Rust 1.85+)
cargo install --git https://github.com/nikuscs/scanr

# Or clone and build
git clone https://github.com/nikuscs/scanr
cd scanr
cargo build --release
```

Pre-built binaries available in [Releases](https://github.com/nikuscs/scanr/releases).

## Quick Start

```bash
# One-time setup (auto-installs PostgreSQL + pgvector if needed)
export OPENAI_API_KEY=sk-...
scanr setup

# Index your project
cd your-project
scanr index

# Search
scanr search "how does authentication work"

# Structural analysis (no setup needed)
scanr scan --mode files

# Quick structure overview
scanr tree
```

## Commands

### `scanr setup`

Create the database, install pgvector extension, and create all tables + HNSW index. Run once per machine.

On macOS, PostgreSQL and pgvector are auto-installed and started via Homebrew if not already present.

```bash
scanr setup                   # Interactive — prompts for PostgreSQL version
scanr setup -y                # Non-interactive — accepts defaults (for agents)
scanr setup --pg-version 17   # Specific PostgreSQL version (default: 18)
```

### `scanr index`

Index or re-index the project. Incremental by default — only files whose content changed since the last run are re-embedded.

```bash
scanr index                          # Index current directory
scanr index --root /path/to/project  # Index a specific project
scanr index --file src/main.rs       # Re-index a single file
scanr index --force                  # Force re-embed everything
scanr index --embedding openai:text-embedding-3-small
scanr index --chunk-size 1500        # Custom chunk size (default: 1000)
scanr index --chunk-overlap 200      # Custom overlap (default: 100)
scanr index --max-bytes 204800       # Skip files larger than this (default: 512 KB)
scanr index --gitignore /path/.gitignore  # Custom gitignore file
```

Use `--embedding <provider:model>` to choose the embedding backend for a new index. Currently supported: `openai:text-embedding-3-large` (default) and `openai:text-embedding-3-small`.

### `scanr search`

Semantic search across the indexed codebase.

```bash
scanr search "payment webhook handler"
scanr search "error handling" --limit 5
scanr search "auth middleware" --lang ts --threshold 0.5
scanr search "database schema" --files-only
scanr search "API routes" --json
```

| Option | Default | Description |
|--------|---------|-------------|
| `--root <path>` | `.` | Project root directory |
| `--limit <n>` | `10` | Number of results |
| `--threshold <n>` | `0.0` | Minimum similarity score (0-1) |
| `--lang <ext>` | — | Filter by language: `ts`, `tsx`, `md`, etc. |
| `--files-only` | — | Unique file paths only, no snippets |
| `--json` | — | Structured JSON: `[{file, language, score, content}]` |

If stale files are detected (changed since last index), a warning is printed to stderr. Agents should detect this and call `index` before searching.

The embedding config is stored per project. `search` always uses the same provider/model that the project was indexed with.

### `scanr tree`

Compact project structure overview for fast orientation.

```bash
scanr tree
scanr tree --path src/commands
scanr tree --depth 4
scanr tree --all
```

| Option | Default | Description |
|--------|---------|-------------|
| `--root <path>` | `.` | Project root directory |
| `--path <subdir>` | — | Focus on a subdirectory within the project |
| `--depth <n>` | `6` | Max branching depth before collapsing subtrees |
| `--inline <n>` | `6` | Max files shown per line before wrapping |
| `--all` | — | Include test directories and test files |

The output is tuned for agents: compact enough to fit in context, but detailed enough to surface the main folders and important top-level files quickly.

### `scanr scan`

Structural analysis of TypeScript/JavaScript codebases. Extracts functions, bindings, and exports — powered by [oxc](https://oxc.rs).

```bash
scanr scan                                    # Scan current directory (compact JSON)
scanr scan --mode verbose                     # Detailed output with spans and metadata
scanr scan --mode files                       # Group functions by file
scanr scan --mode folders                     # Group functions by folder
scanr scan --file src/api.ts                  # Scan a single file
scanr scan --include ts,tsx                   # Only scan specific extensions
scanr scan --exclude vendor,generated         # Exclude directories
scanr scan --function-kinds top               # Only top-level declarations
scanr scan --function-kinds top+arrow         # Declarations + arrow functions
scanr scan --function-kinds top+arrow+class   # Include class methods
```

| Option | Default | Description |
|--------|---------|-------------|
| `--root <path>` | `.` | Project root directory |
| `--mode <mode>` | `compact` | Output format: `compact`, `verbose`, `files`, `folders` |
| `--file <path>` | — | Scan a single file instead of directory |
| `--include <exts>` | `ts,tsx,js,jsx` | File extensions to include (comma-separated) |
| `--exclude <dirs>` | — | Patterns to exclude (comma-separated) |
| `--max-bytes <n>` | `1048576` | Skip files larger than this (bytes) |
| `--function-kinds <filter>` | `all` | Function kinds: `top`, `top+arrow`, `top+arrow+class`, `all` |

**Output modes:**

- **`compact`** — flat JSON arrays optimized for size: `{f: [[file, line, col, name, exported, kind], ...], b: [...], x: [...]}`
- **`verbose`** — pretty-printed JSON with full metadata (spans, async/generator flags, export info)
- **`files`** — functions grouped by file path with dot-notation for nested functions (e.g., `createActions.add`)
- **`folders`** — functions grouped by parent directory with counts

### `scanr status`

Show indexing stats, active embedding config, and stale file count.

```bash
scanr status
scanr status --root /path/to/project
```

### `scanr clear`

Remove all indexed data for a project.

```bash
scanr clear
scanr clear --root /path/to/project
```

### `scanr reindex`

Clear all data and re-index from scratch. Equivalent to `scanr clear && scanr index --force`.

```bash
scanr reindex
scanr reindex --root /path/to/project
scanr reindex --embedding openai:text-embedding-3-small
```

## Supported File Types

| Type | Extensions |
|------|-----------|
| JavaScript/TypeScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.cts`, `.mjs`, `.cjs` |
| Rust | `.rs` |
| Python | `.py` |
| Go | `.go` |
| Markdown | `.md`, `.mdx` |
| Data | `.json`, `.yaml`, `.yml`, `.toml` |

Code files are chunked using tree-sitter (syntax-aware, respects function/class boundaries). Data and markdown files use plain text splitting.

## How It Works

### Semantic Search (index + search)

1. **File discovery** — `git ls-files --cached --others --exclude-standard` filtered by supported extensions, respects `.gitignore`
2. **Parallel chunking** — rayon-parallelized file reading + tree-sitter AST splitting for code, heading-based splitting for markdown (configurable size/overlap)
3. **Deduplication** — SHA-256 content hashing skips unchanged files
4. **Embedding** — OpenAI embeddings, configurable per project (`text-embedding-3-large` by default, or `text-embedding-3-small`), batched (max 100 per call), with exponential backoff retry on 429/5xx
5. **Storage** — pgvector with HNSW index for fast cosine similarity search
6. **Search** — embed query, cosine similarity search, threshold filtering, stale detection

### Structural Scan (scan)

1. **File discovery** — walks project with `.gitignore` support, filters by extension (default: `.ts`, `.tsx`, `.js`, `.jsx`)
2. **Parallel parsing** — rayon-parallelized oxc parsing (native speed, full TypeScript support)
3. **Extraction** — functions (declarations, arrows, class methods, getters/setters), bindings (const/let/var/import/class/enum), exports (named, default, re-exports)
4. **Output** — compact/verbose/files/folders JSON modes, dot-notation nesting for parent-child functions

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | — | Required for embedding |
| `CODE_INDEX_DATABASE_URL` | `postgresql://postgres@localhost/code_index` | PostgreSQL connection URL |
| `SCANR_EMBEDDING` | `openai:text-embedding-3-large` | Default embedding backend for `index` / `reindex` when not already stored for the project |

The embedding configuration used for a project is stored alongside its index metadata. `scanr status` shows the `provider:model` and dimensions currently associated with that project. To switch embeddings for an existing project, run `scanr reindex --embedding ...`.

Override the database URL:

```bash
CODE_INDEX_DATABASE_URL=postgresql://user:pass@host/code_index scanr index
```

## Reading Results

- **>85%** — very strong match, likely exactly what you're looking for
- **70-85%** — relevant context, worth reading
- **<70%** — loosely related, use as breadcrumbs only

If results are poor, try rephrasing the query in terms of what the code *does*, not what you're *looking for* (e.g., "debit wallet integer amount" not "where is money subtracted").

## Claude Code Plugin

This repo ships as a [Claude Code plugin](https://code.claude.com/docs/en/plugins) with a ready-to-use `/scanr:search` skill.

### Install the plugin

```bash
# Add the marketplace
/plugin marketplace add nikuscs/scanr

# Install the plugin
/plugin install scanr
```

### Usage

```bash
/scanr:search                              # check index state, orient
/scanr:search how does auth work           # semantic search
```

> **Requires** the `scanr` binary in your `$PATH`. See [Install](#install) above.

### Manual skill setup

If you prefer not to use the plugin system, copy the skill file into your project:

```bash
mkdir -p .claude/skills/search
cp plugin/skills/search/SKILL.md .claude/skills/search/SKILL.md
```

## AI Agents

If you are an AI agent (Claude Code, Claude.ai, OpenAI Codex, or any tool-calling agent), you can use `scanr` to semantically search any codebase. Download the binary and call it directly from your tool/shell integration.

### Quick setup

```bash
# Download the pre-compiled binary for your platform from Releases
# https://github.com/nikuscs/scanr/releases/latest

# One-time setup (non-interactive for agents)
scanr setup -y

# Index the project
scanr index

# Search
scanr search "how does authentication work" --json
```

### Tips for agents

- Use `--json` for structured output you can parse: `[{file, language, score, content}]`
- Use `tree` first for fast orientation before semantic search
- Use `--files-only` for quick orientation before deep-reading files
- Use `--limit` to control result count and stay within context limits
- Run `scanr status` to check if the index is stale before searching
- After editing files, re-index the changed file: `scanr index --file <path>`
- Phrase queries in terms of what the code *does*, not what you're looking for
- Use `-y` flag on `setup` to skip interactive prompts

## Credits

scanr is built on top of excellent open-source projects:

- [oxc](https://oxc.rs) — Blazing-fast JavaScript/TypeScript parser and linter, powers the `scan` command
- [tree-sitter](https://tree-sitter.github.io/tree-sitter/) — Incremental parsing for syntax-aware code chunking
- [pgvector](https://github.com/pgvector/pgvector) — Open-source vector similarity search for PostgreSQL
- [sqlx](https://github.com/launchbadge/sqlx) — Async Rust SQL toolkit with compile-time checked queries
- [clap](https://github.com/clap-rs/clap) — Command-line argument parser for Rust
- [rayon](https://github.com/rayon-rs/rayon) — Data parallelism library for Rust
- [ignore](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) — `.gitignore`-aware file walking (from the ripgrep project)

Embeddings are currently generated via the [OpenAI API](https://platform.openai.com/docs/guides/embeddings) using either `text-embedding-3-large` (2000 dimensions, default) or `text-embedding-3-small` (1536 dimensions). Vectors are stored in a `vector(2000)` column — smaller models are zero-padded, which preserves cosine similarity.

## Roadmap

- [ ] Configurable embedding backends instead of a single hardcoded provider
- [ ] Support for local embedding models, so semantic search can run without OpenAI
- [ ] Better tradeoffs between cost, speed, and quality for indexing large repositories
- [ ] Additional structural analysis beyond the current TypeScript/JavaScript scan
- [ ] Broader language-aware retrieval improvements on top of the current chunking pipeline

## Related Projects

- [lauyer](https://github.com/nikuscs/lauyer) — CLI for Portuguese legal jurisprudence search
- [crauler](https://github.com/nikuscs/crauler) — Web crawler with social media extraction
- [ts-code-scan](https://github.com/nikuscs/ts-code-scan) — TypeScript/JavaScript codebase indexer

## License

[MIT](LICENSE)
