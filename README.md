# scanr

[![CI](https://github.com/nikuscs/scanr/actions/workflows/ci.yml/badge.svg)](https://github.com/nikuscs/scanr/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/nikuscs/scanr)](https://github.com/nikuscs/scanr/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Fast semantic codebase search via OpenAI embeddings + pgvector. Works as a skill for [Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Claude.ai](https://claude.ai), [OpenAI Codex](https://openai.com/index/openai-codex/), and any AI agent.**

Walks your git repo, chunks files with tree-sitter (syntax-aware splitting), embeds via `text-embedding-3-large` at 3072 dimensions, and stores in PostgreSQL with pgvector for instant cosine-similarity search.

> **Note:** This project was built with AI assistance. Review, test, and verify before using in production.

## Why?

- **Semantic search** — find code by meaning, not just keywords
- **Incremental** — SHA-256 hashing skips unchanged files, safe to re-run
- **Fast** — parallel file chunking via rayon, HNSW index, batch embeddings (100/call)
- **Zero config** — auto-installs PostgreSQL + pgvector via Homebrew, auto-creates DB and schema
- **Multi-language** — tree-sitter grammars for TypeScript, JavaScript, Rust, Python, Go + plain chunking for JSON/YAML/TOML/Markdown
- **Resilient** — exponential backoff retry on OpenAI 429/5xx errors
- **Agent-friendly** — `--json` and `--files-only` output modes, `-y` for non-interactive setup, stale warnings to stderr

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
scanr index --chunk-size 1500        # Custom chunk size (default: 1000)
scanr index --chunk-overlap 200      # Custom overlap (default: 100)
scanr index --gitignore /path/.gitignore  # Custom gitignore file
```

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

### `scanr status`

Show indexing stats and stale file count.

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

1. **File discovery** — `git ls-files --cached --others --exclude-standard` filtered by supported extensions, respects `.gitignore`
2. **Parallel chunking** — rayon-parallelized file reading + tree-sitter AST splitting for code, heading-based splitting for markdown (configurable size/overlap)
3. **Deduplication** — SHA-256 content hashing skips unchanged files
4. **Embedding** — OpenAI `text-embedding-3-large` at 3072 dimensions, batched (max 100 per call), with exponential backoff retry on 429/5xx
5. **Storage** — pgvector with HNSW index for fast cosine similarity search
6. **Search** — embed query, cosine similarity search, threshold filtering, stale detection

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | — | Required for embedding |
| `CODE_INDEX_DATABASE_URL` | `postgresql://postgres@localhost/code_index` | PostgreSQL connection URL |

Override the database URL:

```bash
CODE_INDEX_DATABASE_URL=postgresql://user:pass@host/code_index scanr index
```

## Reading Results

- **>85%** — very strong match, likely exactly what you're looking for
- **70-85%** — relevant context, worth reading
- **<70%** — loosely related, use as breadcrumbs only

If results are poor, try rephrasing the query in terms of what the code *does*, not what you're *looking for* (e.g., "debit wallet integer amount" not "where is money subtracted").

## AI Agents

If you are an AI agent (Claude Code, Claude.ai, OpenAI Codex, or any tool-calling agent), you can use `scanr` as a skill to semantically search any codebase. Download the binary and call it directly from your tool/shell integration. A `SKILL.md` file is included for Claude Code skill registration.

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

### Searching

```bash
# Semantic search with JSON output (best for agents)
scanr search "payment processing" --json --limit 5

# Files only (orientation/discovery)
scanr search "database models" --files-only

# Filter by language
scanr search "API routes" --lang ts --json

# Check if index is stale
scanr status
```

### Tips for agents

- Use `--json` for structured output you can parse: `[{file, language, score, content}]`
- Use `--files-only` for quick orientation before deep-reading files
- Use `--limit` to control result count and stay within context limits
- Run `scanr status` to check if the index is stale before searching
- If stale files are detected, run `scanr index` before searching
- After editing files, re-index the changed file: `scanr index --file <path>`
- Phrase queries in terms of what the code *does*, not what you're looking for
- Use `-y` flag on `setup` to skip interactive prompts

## Related Projects

- [lauyer](https://github.com/nikuscs/lauyer) — CLI for Portuguese legal jurisprudence search
- [crauler](https://github.com/nikuscs/crauler) — Web crawler with social media extraction
- [ts-code-scan](https://github.com/nikuscs/ts-code-scan) — TypeScript/JavaScript codebase indexer

## License

[MIT](LICENSE)
