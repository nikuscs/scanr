# scanr

Fast semantic codebase search via OpenAI embeddings + pgvector.

Walks your git repo, chunks files with tree-sitter, embeds via `text-embedding-3-large`, and stores in PostgreSQL with pgvector for instant cosine-similarity search.

## Install

Download the latest binary from [Releases](https://github.com/nikuscs/scanr/releases), or build from source:

```bash
cargo install --path .
```

## Requirements

- PostgreSQL with pgvector extension
- `OPENAI_API_KEY` environment variable

PostgreSQL is auto-installed via Homebrew if missing (macOS). The database and schema are created automatically on first run.

## Quick Start

```bash
# Setup database (auto-creates DB + tables + pgvector extension)
scanr setup

# Index your project
cd your-project
scanr index

# Search
scanr search "how does authentication work"
```

## Commands

### `setup`

Create the database, install pgvector extension, and create all tables + HNSW index. Run once per machine.

```bash
scanr setup
```

### `index`

Index or re-index the project. Incremental by default — only changed files are re-embedded.

```bash
scanr index                          # Index current directory
scanr index --root /path/to/project  # Index a specific project
scanr index --file src/main.rs       # Re-index a single file
scanr index --force                  # Force re-embed everything
```

### `search`

Semantic search across the indexed codebase.

```bash
scanr search "payment webhook handler"
scanr search "error handling" --limit 5
scanr search "auth middleware" --lang ts --threshold 0.5
scanr search "database schema" --files-only
scanr search "API routes" --json
```

Options:
- `--root <path>` — Project root (default: `.`)
- `--limit <n>` — Number of results (default: `10`)
- `--threshold <n>` — Minimum similarity score 0–1 (default: `0.0`)
- `--lang <ext>` — Filter by language: `ts`, `tsx`, `md`, etc.
- `--files-only` — Unique file paths only, no snippets
- `--json` — Structured JSON output: `[{file, language, score, content}]`

### `status`

Show indexing stats and stale file count.

```bash
scanr status
scanr status --root /path/to/project
```

### `clear`

Remove all indexed data for a project.

```bash
scanr clear
scanr clear --root /path/to/project
```

## Supported File Types

**Code:** `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.cts`, `.mjs`, `.cjs`
**Docs:** `.md`, `.mdx`

## How It Works

1. **File discovery** — `git ls-files --cached --others --exclude-standard` filtered by supported extensions
2. **Chunking** — Tree-sitter AST-based splitting for code, heading-based splitting for markdown (1000 chars, 100 overlap)
3. **Deduplication** — SHA-256 content hashing skips unchanged files
4. **Embedding** — OpenAI `text-embedding-3-large` at 3072 dimensions, batched (max 100 per API call)
5. **Storage** — pgvector with HNSW index for fast cosine similarity search
6. **Search** — Embed query, cosine similarity search, threshold filtering, stale detection

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | — | Required for embedding |
| `CODE_INDEX_DATABASE_URL` | `postgresql://postgres@localhost/code_index` | PostgreSQL connection |

## Reading Results

- **>85%** — Very strong match, likely exactly what you're looking for
- **70–85%** — Relevant context, worth reading
- **<70%** — Loosely related, use as breadcrumbs

## License

MIT
