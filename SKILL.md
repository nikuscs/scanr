---
name: code-index
description: Semantic codebase search. Index the project into pgvector and search by meaning using OpenAI embeddings. Use when exploring unfamiliar code, finding patterns, locating where a concept is implemented, or before implementing a feature.
argument-hint: [search query]
allowed-tools: Bash(scanr *)
metadata: {"openclaw":{"emoji":"🔍","requires":{"bins":["scanr"]},"install":[{"id":"binary","kind":"custom","command":"# Download from https://github.com/nikuscs/scanr/releases/latest\n# macOS: scanr-macos-arm64.tar.gz\n# Linux x64: scanr-linux-x64.tar.gz\ntar -xzf scanr-*.tar.gz && chmod +x scanr && sudo mv scanr /usr/local/bin/","label":"Download pre-built binary (recommended)"},{"id":"cargo","kind":"cargo","crate":"scanr","bins":["scanr"],"label":"Install via Cargo (requires Rust)"}]}}
---

# Code Index Skill

Semantic search over the codebase using OpenAI `text-embedding-3-large` embeddings + pgvector. Outputs ranked results optimized for LLM consumption.

## Current index state

!`scanr status 2>/dev/null || echo "Not indexed — run setup and index first"`

## Quick action

If `$ARGUMENTS` is provided, search immediately:

```bash
scanr search "$ARGUMENTS" --json --limit 10
```

If no arguments, check the index state above and decide what to do based on the user's request.

## Installation

**No Rust or compilation required.** Download the pre-built binary for your platform from [Releases](https://github.com/nikuscs/scanr/releases/latest):

### macOS (Apple Silicon)

```bash
curl -L https://github.com/nikuscs/scanr/releases/latest/download/scanr-macos-arm64.tar.gz | tar xz
chmod +x scanr
sudo mv scanr /usr/local/bin/
```

### Linux (x64)

```bash
curl -L https://github.com/nikuscs/scanr/releases/latest/download/scanr-linux-x64.tar.gz | tar xz
chmod +x scanr
sudo mv scanr /usr/local/bin/
```

### Verify

```bash
scanr --version
```

## Prerequisites

`OPENAI_API_KEY` must be set in the environment, `~/.zshrc`, `~/.bashrc`, or a `.env` file in the project (walks up to 6 directories).

## When to use (trigger phrases)

Use this skill when the user asks:

- "find where X is implemented"
- "how does X work in this codebase"
- "search for code that does X"
- "find similar patterns to X"
- "where is X defined"
- "what files handle X"
- Any request to explore, understand, or navigate an unfamiliar codebase by meaning rather than keywords

## First-time setup (once per machine)

```bash
scanr setup -y
```

The `-y` flag skips prompts (non-interactive). This auto-installs PostgreSQL and pgvector via Homebrew if missing (macOS), creates the database, installs the vector extension, and creates all tables + HNSW index. Use `--pg-version N` to pick a specific PostgreSQL version (default: 18).

## Indexing

```bash
# Index the current project (incremental — only changed files)
scanr index

# Re-index a single file after editing
scanr index --file <relative-path>

# Force re-embed everything
scanr index --force

# Custom chunk size/overlap
scanr index --chunk-size 1500 --chunk-overlap 200

# Custom gitignore file
scanr index --gitignore /path/.gitignore

# Specific project root
scanr index --root /path/to/project
```

## Searching

```bash
# Basic search
scanr search "how does authentication work"

# JSON output (best for agents)
scanr search "payment processing" --json --limit 5

# Files only (orientation/discovery)
scanr search "database models" --files-only

# Filter by language
scanr search "API routes" --lang ts --threshold 0.5
```

### Search Options

| Flag | Description | Example |
|------|-------------|---------|
| `--limit N` | Number of results (default: 10) | `--limit 5` |
| `--threshold N` | Minimum similarity 0-1 (default: 0) | `--threshold 0.5` |
| `--lang <ext>` | Filter by extension | `--lang ts` |
| `--files-only` | Unique file paths only, no snippets | `--files-only` |
| `--json` | Structured JSON output | `--json` |
| `--root <path>` | Project root if not cwd | `--root /path/to/project` |

If stale files are detected (changed since last index), a warning prints to stderr. When you see this, run `index` before searching again.

## Other commands

```bash
scanr status          # Show files, chunks, stale count
scanr clear           # Remove all indexed data for this project
scanr reindex         # Clear + force re-index from scratch
```

## Supported file types

JS/TS (`.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.cts`, `.mjs`, `.cjs`), Rust (`.rs`), Python (`.py`), Go (`.go`), Markdown (`.md`, `.mdx`), Data (`.json`, `.yaml`, `.yml`, `.toml`).

Code files are chunked using tree-sitter (syntax-aware). Data and markdown use plain text splitting. File discovery respects `.gitignore`.

## Reading results

- **>85%** — very strong match, likely exactly what you need
- **70-85%** — relevant context, worth reading
- **<70%** — loosely related, use as breadcrumbs

Phrase queries in terms of what the code *does*, not what you're looking for. Example: "debit wallet integer amount" not "where is money subtracted".

## Agent Guidelines

### Best Practices

1. **Use `--json`** for structured output you can parse: `[{file, language, score, content}]`
2. **Use `--files-only`** for quick orientation before deep-reading files
3. **Use `--limit 5-10`** to stay within context limits
4. **Check the index state** injected above — if stale or not indexed, run `index` first
5. **After editing files**, re-index the changed file: `scanr index --file <path>`
6. **Use `-y`** on `setup` to skip interactive prompts

### Common Workflows

#### Explore an unfamiliar codebase

```bash
# 1. Setup and index (first time only)
scanr setup -y
scanr index

# 2. Orientation — what files exist for a concept
scanr search "authentication" --files-only

# 3. Deep dive — get code snippets
scanr search "how does JWT token validation work" --json --limit 5

# 4. Read the returned files with the Read tool for full context
```

#### Find patterns before implementing

```bash
# Find existing patterns to follow
scanr search "how are API endpoints structured" --lang ts --limit 5

# Find similar implementations
scanr search "payment webhook handler" --json
```

#### Keep index fresh during development

```bash
# After editing a file
scanr index --file src/auth/login.ts

# After major changes
scanr reindex
```

## Environment variables

- `OPENAI_API_KEY` — required for embedding (reads from env, `~/.zshrc`, or closest `.env`)
- `CODE_INDEX_DATABASE_URL` — override database URL (default: `postgresql://postgres@localhost/code_index`)
