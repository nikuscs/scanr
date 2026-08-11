# Real-world test checklist

## CLI

- [ ] `scanr --help` lists `search`, `scan`, `tree`, and `dupes`
- [ ] `scanr search --help` documents content/path search flags
- [ ] `scanr scan --help` documents output modes and rules
- [ ] `scanr tree --help` documents tree controls
- [ ] `scanr dupes --help` documents similarity controls

## Search

- [ ] Content results match `rg -n` for the same literal pattern
- [ ] JSON output is stable and sorted by path then line
- [ ] Path mode respects ignored files
- [ ] `--ignore-case`, `--glob`, `--max-count`, and `--context` work together

## Scan

- [ ] TypeScript and TSX files parse without panics
- [ ] Functions, bindings, references, and exports remain stable
- [ ] Compact, verbose, files, and folders modes serialize deterministically
- [ ] Rules report expected violations
- [ ] Nested functions expose parent and capture information

## Tree

- [ ] Ignored and hidden directories are omitted
- [ ] Test paths are omitted by default and included with `--all`
- [ ] Natural sorting and depth collapsing remain stable

## Similarity

- [ ] Function and type comparisons match the upstream regression fixtures
- [ ] Results are deterministic across runs

## Offline guarantee

- [ ] The binary performs no network calls
- [ ] No database or service is required
