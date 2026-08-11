# 📡 scanr

[![Release](https://img.shields.io/github/v/release/nikuscs/scanr)](https://github.com/nikuscs/scanr/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A fast, deterministic static-analysis and search CLI for TypeScript and JavaScript projects. `scanr` is a single local binary: it needs no database, network access, service, or API key.

## Features

- **Structural analysis** — extract functions, bindings, references, and exports with oxc
- **Content and path search** — gitignore-aware, parallel grep with stable JSON output
- **Project tree** — compact repository structure for quick orientation
- **Agent-friendly** — deterministic output suitable for scripts and coding agents

## Install

```bash
cargo install --git https://github.com/nikuscs/scanr
```

Or build from source with Rust 1.85+:

```bash
git clone https://github.com/nikuscs/scanr
cd scanr
cargo build --release
```

## Quick start

```bash
scanr search "useState" --root . --json
scanr scan --root . --mode files
scanr tree --root .
```

## Commands

### `scanr search`

Search file contents with a literal pattern:

```bash
scanr search "useState"
scanr search "todo" --ignore-case --glob '*.ts' --context 2
scanr search "handler" --max-count 5 --json
```

Search paths instead of contents:

```bash
scanr search --path "components"
scanr search --path '*.test.ts'
```

The walker respects `.gitignore`. Results are sorted by path and line, including when files are searched in parallel.

### `scanr scan`

Extract TypeScript and JavaScript structure:

```bash
scanr scan
scanr scan --root apps/web --mode verbose
scanr scan --file src/index.ts
scanr scan --mode files
scanr scan --mode folders
```

Flags:

- `--root <path>` — project root
- `--mode compact|verbose|files|folders` — output shape
- `--include ts,tsx,js,jsx,...` — included extensions
- `--exclude <patterns>` — excluded paths
- `--max-bytes <bytes>` — maximum file size
- `--function-kinds top|top+arrow|top+arrow+class|all` — function categories
- `--file <path>` — scan one file

### `scanr tree`

Print a compact project structure:

```bash
scanr tree
scanr tree --path src/commands
scanr tree --depth 4 --inline 8
scanr tree --all
```

## Development

```bash
cargo test
cargo build --release
```

## License

MIT
