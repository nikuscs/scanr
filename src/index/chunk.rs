use anyhow::Result;

/// Find the nearest char boundary at or after `idx` in `s`.
fn ceil_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

pub struct ChunkConfig {
    pub size: usize,
    pub overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self { size: 1000, overlap: 100 }
    }
}

pub fn chunk_code(source: &str, ext: &str, config: &ChunkConfig) -> Result<Vec<String>> {
    let language = match ext {
        "ts" | "mts" | "cts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "js" | "mjs" | "cjs" | "jsx" => tree_sitter_javascript::LANGUAGE.into(),
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "py" => tree_sitter_python::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        _ => return Ok(chunk_plain(source, config)),
    };

    chunk_with_tree_sitter(source, language, ext, config)
}

pub fn chunk_markdown(source: &str, config: &ChunkConfig) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in source.lines() {
        let is_heading = line.starts_with('#');

        if is_heading && !current.trim().is_empty() {
            chunks.push(current.trim().to_string());
            current = String::new();
        }

        current.push_str(line);
        current.push('\n');

        if current.len() >= config.size && !is_heading {
            chunks.push(current.trim().to_string());
            let overlap_start =
                ceil_char_boundary(&current, current.len().saturating_sub(config.overlap));
            current = current[overlap_start..].to_string();
        }
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    chunks
}

fn chunk_with_tree_sitter(
    source: &str,
    language: tree_sitter::Language,
    ext: &str,
    config: &ChunkConfig,
) -> Result<Vec<String>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language)?;

    let Some(tree) = parser.parse(source, None) else {
        return Ok(chunk_plain(source, config));
    };

    let root = tree.root_node();
    let mut chunks = Vec::new();
    let mut current = String::new();

    collect_nodes(root, source, &mut chunks, &mut current, ext, config);

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        return Ok(chunk_plain(source, config));
    }

    Ok(chunks)
}

fn collect_nodes(
    node: tree_sitter::Node,
    source: &str,
    chunks: &mut Vec<String>,
    current: &mut String,
    ext: &str,
    config: &ChunkConfig,
) {
    if is_chunk_boundary(&node, ext) {
        let text = &source[node.start_byte()..node.end_byte()];

        if !current.trim().is_empty() && current.len() + text.len() > config.size {
            chunks.push(current.trim().to_string());
            let overlap_start =
                ceil_char_boundary(current, current.len().saturating_sub(config.overlap));
            *current = current[overlap_start..].to_string();
        }

        current.push_str(text);
        current.push('\n');

        if current.len() >= config.size {
            chunks.push(current.trim().to_string());
            let overlap_start =
                ceil_char_boundary(current, current.len().saturating_sub(config.overlap));
            *current = current[overlap_start..].to_string();
        }

        return;
    }

    let child_count = node.child_count();
    if child_count == 0 {
        let text = &source[node.start_byte()..node.end_byte()];
        current.push_str(text);
        return;
    }

    for i in 0..child_count as u32 {
        if let Some(child) = node.child(i) {
            collect_nodes(child, source, chunks, current, ext, config);
        }
    }
}

fn is_chunk_boundary(node: &tree_sitter::Node, ext: &str) -> bool {
    let depth = {
        let mut d = 0u32;
        let mut n = *node;
        while let Some(parent) = n.parent() {
            d += 1;
            n = parent;
        }
        d
    };

    if depth > 2 {
        return false;
    }

    match ext {
        "rs" => matches!(
            node.kind(),
            "function_item"
                | "impl_item"
                | "struct_item"
                | "enum_item"
                | "trait_item"
                | "mod_item"
                | "use_declaration"
                | "const_item"
                | "static_item"
                | "type_item"
                | "macro_definition"
        ),
        "py" => matches!(
            node.kind(),
            "function_definition"
                | "class_definition"
                | "decorated_definition"
                | "import_statement"
                | "import_from_statement"
                | "expression_statement"
        ),
        "go" => matches!(
            node.kind(),
            "function_declaration"
                | "method_declaration"
                | "type_declaration"
                | "import_declaration"
                | "var_declaration"
                | "const_declaration"
        ),
        _ => matches!(
            node.kind(),
            "function_declaration"
                | "method_definition"
                | "class_declaration"
                | "interface_declaration"
                | "type_alias_declaration"
                | "enum_declaration"
                | "export_statement"
                | "lexical_declaration"
                | "variable_declaration"
                | "import_statement"
                | "expression_statement"
        ),
    }
}

pub fn chunk_plain(source: &str, config: &ChunkConfig) -> Vec<String> {
    let mut chunks = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut start = 0;

    while start < lines.len() {
        let mut end = start;
        let mut len = 0;

        while end < lines.len() && len + lines[end].len() < config.size {
            len += lines[end].len() + 1;
            end += 1;
        }

        if end == start {
            end = start + 1;
        }

        let chunk: String = lines[start..end].join("\n");
        if !chunk.trim().is_empty() {
            chunks.push(chunk);
        }

        let overlap_lines = config.overlap / 40;
        let next = if end > overlap_lines { end - overlap_lines } else { end };
        // Ensure we always advance to avoid infinite loops
        start = if next <= start { end } else { next };
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ChunkConfig {
        ChunkConfig { size: 200, overlap: 50 }
    }

    #[test]
    fn plain_chunk_short_content() {
        let content = "line one\nline two\nline three";
        let config = ChunkConfig { size: 1000, overlap: 0 };
        let chunks = chunk_plain(content, &config);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("line one"));
        assert!(chunks[0].contains("line three"));
    }

    #[test]
    fn plain_chunk_splits_large_content() {
        let content =
            (0..100).map(|i| format!("this is line number {i}")).collect::<Vec<_>>().join("\n");
        let chunks = chunk_plain(&content, &default_config());
        assert!(chunks.len() > 1, "expected multiple chunks, got {}", chunks.len());
    }

    #[test]
    fn plain_chunk_empty_content() {
        let chunks = chunk_plain("", &default_config());
        assert!(chunks.is_empty());
    }

    #[test]
    fn plain_chunk_whitespace_only() {
        let chunks = chunk_plain("   \n  \n   ", &default_config());
        assert!(chunks.is_empty());
    }

    #[test]
    fn markdown_heading_splits() {
        let content = "# Heading 1\nSome text.\n# Heading 2\nMore text.";
        let chunks = chunk_markdown(content, &ChunkConfig { size: 1000, overlap: 100 });
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].starts_with("# Heading 1"));
        assert!(chunks[1].starts_with("# Heading 2"));
    }

    #[test]
    fn markdown_single_section() {
        let content = "# Title\nJust one section with content.";
        let chunks = chunk_markdown(content, &default_config());
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn chunk_code_typescript() {
        let content = r#"
function hello() {
    console.log("hello");
}

function world() {
    console.log("world");
}
"#;
        let chunks = chunk_code(content, "ts", &ChunkConfig { size: 50, overlap: 10 }).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_code_rust() {
        let content = r#"
fn main() {
    println!("hello");
}

fn other() {
    println!("world");
}
"#;
        let chunks = chunk_code(content, "rs", &ChunkConfig { size: 200, overlap: 20 }).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_code_python() {
        let content = r#"
def hello():
    print("hello")

def world():
    print("world")
"#;
        let chunks = chunk_code(content, "py", &ChunkConfig { size: 200, overlap: 20 }).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_code_go() {
        let content = r#"
package main

func hello() {
    fmt.Println("hello")
}

func world() {
    fmt.Println("world")
}
"#;
        let chunks = chunk_code(content, "go", &ChunkConfig { size: 200, overlap: 20 }).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_code_unknown_ext_falls_back_to_plain() {
        let content = "some content\nmore lines\n";
        let chunks = chunk_code(content, "xyz", &default_config()).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_code_javascript() {
        let content = r#"
function greet(name) {
    return "Hello, " + name;
}

class Greeter {
    constructor(name) {
        this.name = name;
    }
}
"#;
        let chunks = chunk_code(content, "js", &ChunkConfig { size: 80, overlap: 10 }).unwrap();
        assert!(!chunks.is_empty());
    }

    // --- New edge-case tests ---

    #[test]
    fn plain_chunk_overlap_carries_content_between_chunks() {
        // Build content large enough to force multiple chunks
        let content = (0..100).map(|i| format!("line-{i:03}")).collect::<Vec<_>>().join("\n");
        let config = ChunkConfig { size: 200, overlap: 120 }; // overlap = 3 lines worth (120/40)

        let chunks = chunk_plain(&content, &config);
        assert!(chunks.len() >= 2, "need at least 2 chunks, got {}", chunks.len());

        // The last few lines of chunk N should appear at the start of chunk N+1
        for pair in chunks.windows(2) {
            let prev_lines: Vec<&str> = pair[0].lines().collect();
            let next_lines: Vec<&str> = pair[1].lines().collect();

            // At least one line from the tail of the previous chunk should appear
            // at the head of the next chunk (overlap)
            let tail = prev_lines.last().unwrap();
            assert!(
                next_lines.iter().any(|l| l == tail),
                "expected overlap: tail line {:?} of prev chunk not found in next chunk {:?}",
                tail,
                &next_lines[..next_lines.len().min(5)]
            );
        }
    }

    #[test]
    fn plain_chunk_very_long_single_line() {
        // A single line with no newlines that exceeds chunk_size
        let long_line = "x".repeat(5000);
        let config = ChunkConfig { size: 200, overlap: 0 };
        let chunks = chunk_plain(&long_line, &config);

        // Should still produce at least one chunk containing the full line
        assert_eq!(chunks.len(), 1, "single long line should produce exactly 1 chunk");
        assert_eq!(chunks[0].len(), 5000);
    }

    #[test]
    fn plain_chunk_multiple_long_lines() {
        // Several lines, each exceeding chunk_size — each becomes its own chunk
        let lines: Vec<String> = (0..4).map(|i| format!("{}{}", i, "y".repeat(300))).collect();
        let content = lines.join("\n");
        let config = ChunkConfig { size: 200, overlap: 0 };
        let chunks = chunk_plain(&content, &config);

        assert!(
            chunks.len() >= 4,
            "each long line should force a chunk boundary, got {} chunks",
            chunks.len()
        );
    }

    #[test]
    fn chunk_code_markdown_md_heading_splitting() {
        let content = "\
# Introduction
This is the intro section with enough text to matter.

## Getting Started
Steps to get started with the project.

## API Reference
Details about the API endpoints and usage.

### Nested heading
Some nested content here.
";
        // .md falls through the tree-sitter match to chunk_plain
        // but let's verify chunk_code returns something sensible
        let chunks = chunk_code(content, "md", &ChunkConfig { size: 1000, overlap: 0 }).unwrap();
        assert!(!chunks.is_empty());
        // All the content should be present across the chunks
        let joined = chunks.join("\n");
        assert!(joined.contains("Introduction"));
        assert!(joined.contains("API Reference"));
    }

    #[test]
    fn chunk_code_markdown_mdx_heading_splitting() {
        let content = "\
# Component Docs

Some description.

## Props

| Prop | Type |
|------|------|
| name | string |

## Examples

```jsx
<MyComponent name=\"hello\" />
```
";
        let chunks = chunk_code(content, "mdx", &ChunkConfig { size: 1000, overlap: 0 }).unwrap();
        assert!(!chunks.is_empty());
        let joined = chunks.join("\n");
        assert!(joined.contains("Component Docs"));
        assert!(joined.contains("Examples"));
    }

    #[test]
    fn chunk_markdown_heading_splitting_small_size() {
        let content = "\
# Section A
Some text for section A that is moderately long to push boundaries.

# Section B
Text for section B.

# Section C
Text for section C.
";
        let chunks = chunk_markdown(content, &ChunkConfig { size: 60, overlap: 0 });
        assert!(
            chunks.len() >= 3,
            "expected at least 3 chunks from 3 headings, got {}",
            chunks.len()
        );
        assert!(chunks[0].starts_with("# Section A"));
        assert!(chunks.iter().any(|c| c.contains("Section B")));
        assert!(chunks.iter().any(|c| c.contains("Section C")));
    }

    #[test]
    fn chunk_code_deeply_nested_rust_functions() {
        let content = r#"
fn outer() {
    fn inner_a() {
        fn deeply_nested() {
            println!("deep");
        }
        deeply_nested();
    }

    fn inner_b() {
        for i in 0..10 {
            if i > 5 {
                println!("{}", i);
            }
        }
    }

    inner_a();
    inner_b();
}

fn standalone() {
    println!("standalone");
}
"#;
        let chunks = chunk_code(content, "rs", &ChunkConfig { size: 80, overlap: 10 }).unwrap();
        assert!(!chunks.is_empty());
        // Nested functions should be part of their parent's chunk, not split separately
        // (is_chunk_boundary checks depth <= 2)
        let joined = chunks.join("\n");
        assert!(joined.contains("deeply_nested"));
        assert!(joined.contains("standalone"));
    }

    #[test]
    fn chunk_code_deeply_nested_typescript() {
        let content = r#"
function outer() {
    function middleA() {
        function innerDeep() {
            console.log("deep");
        }
        innerDeep();
    }

    function middleB() {
        for (let i = 0; i < 10; i++) {
            if (i > 5) {
                console.log(i);
            }
        }
    }

    middleA();
    middleB();
}

function topLevel() {
    console.log("top");
}
"#;
        let chunks = chunk_code(content, "ts", &ChunkConfig { size: 100, overlap: 10 }).unwrap();
        assert!(!chunks.is_empty());
        let joined = chunks.join("\n");
        assert!(joined.contains("innerDeep"));
        assert!(joined.contains("topLevel"));
    }

    #[test]
    fn chunk_code_deeply_nested_python() {
        let content = r#"
def outer():
    def middle():
        def deep():
            print("deep")
        deep()

    def another():
        for i in range(10):
            if i > 5:
                print(i)

    middle()
    another()

def standalone():
    print("standalone")
"#;
        let chunks = chunk_code(content, "py", &ChunkConfig { size: 100, overlap: 10 }).unwrap();
        assert!(!chunks.is_empty());
        let joined = chunks.join("\n");
        assert!(joined.contains("deep"));
        assert!(joined.contains("standalone"));
    }

    #[test]
    fn tree_sitter_single_function_larger_than_chunk_size() {
        // A single function that far exceeds the chunk_size should still appear as a chunk
        let body_lines: Vec<String> =
            (0..50).map(|i| format!("    println!(\"line {i}\");")).collect();
        let content = format!("fn big_function() {{\n{}\n}}\n", body_lines.join("\n"));

        let config = ChunkConfig { size: 100, overlap: 10 };
        let chunks = chunk_code(&content, "rs", &config).unwrap();

        // The function must not be lost; it should appear in at least one chunk
        assert!(!chunks.is_empty());
        let joined = chunks.join("\n");
        assert!(
            joined.contains("big_function"),
            "the oversized function should still be included in output"
        );
    }

    #[test]
    fn tree_sitter_single_large_ts_function() {
        let body_lines: Vec<String> =
            (0..50).map(|i| format!("    console.log(\"line {i}\");")).collect();
        let content = format!("function hugeFn() {{\n{}\n}}\n", body_lines.join("\n"));

        let config = ChunkConfig { size: 100, overlap: 10 };
        let chunks = chunk_code(&content, "ts", &config).unwrap();
        assert!(!chunks.is_empty());
        let joined = chunks.join("\n");
        assert!(joined.contains("hugeFn"));
    }

    #[test]
    fn empty_file_typescript() {
        let chunks = chunk_code("", "ts", &default_config()).unwrap();
        assert!(chunks.is_empty(), "empty TS file should produce no chunks");
    }

    #[test]
    fn empty_file_rust() {
        let chunks = chunk_code("", "rs", &default_config()).unwrap();
        assert!(chunks.is_empty(), "empty Rust file should produce no chunks");
    }

    #[test]
    fn empty_file_python() {
        let chunks = chunk_code("", "py", &default_config()).unwrap();
        assert!(chunks.is_empty(), "empty Python file should produce no chunks");
    }

    #[test]
    fn empty_file_go() {
        let chunks = chunk_code("", "go", &default_config()).unwrap();
        assert!(chunks.is_empty(), "empty Go file should produce no chunks");
    }

    #[test]
    fn empty_file_javascript() {
        let chunks = chunk_code("", "js", &default_config()).unwrap();
        assert!(chunks.is_empty(), "empty JS file should produce no chunks");
    }

    #[test]
    fn empty_file_markdown() {
        let chunks = chunk_markdown("", &default_config());
        assert!(chunks.is_empty(), "empty markdown should produce no chunks");
    }

    #[test]
    fn chunk_code_json_falls_back_to_plain() {
        let content = r#"{
    "name": "scanr",
    "version": "0.1.0",
    "dependencies": {
        "serde": "1.0",
        "tokio": "1.0"
    }
}"#;
        let chunks = chunk_code(content, "json", &ChunkConfig { size: 1000, overlap: 0 }).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("scanr"));
        assert!(chunks[0].contains("dependencies"));
    }

    #[test]
    fn chunk_code_yaml_falls_back_to_plain() {
        let content = "\
name: scanr
version: 0.1.0
dependencies:
  serde: 1.0
  tokio: 1.0
settings:
  debug: true
  log_level: info
";
        let chunks = chunk_code(content, "yaml", &ChunkConfig { size: 1000, overlap: 0 }).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("scanr"));
        assert!(chunks[0].contains("settings"));
    }

    #[test]
    fn chunk_code_toml_falls_back_to_plain() {
        let content = "\
[package]
name = \"scanr\"
version = \"0.1.0\"

[dependencies]
serde = \"1.0\"
tokio = \"1.0\"
";
        let chunks = chunk_code(content, "toml", &ChunkConfig { size: 1000, overlap: 0 }).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("scanr"));
        assert!(chunks[0].contains("dependencies"));
    }

    #[test]
    fn chunk_code_json_large_splits_into_multiple() {
        // Large JSON should still be split by chunk_plain
        let entries: Vec<String> =
            (0..100).map(|i| format!("    \"key_{i}\": \"value_{i}\"")).collect();
        let content = format!("{{\n{}\n}}", entries.join(",\n"));
        let config = ChunkConfig { size: 200, overlap: 0 };
        let chunks = chunk_code(&content, "json", &config).unwrap();
        assert!(
            chunks.len() > 1,
            "large JSON should split into multiple chunks, got {}",
            chunks.len()
        );
    }

    #[test]
    fn chunk_code_yaml_large_splits_into_multiple() {
        let entries: Vec<String> = (0..100).map(|i| format!("key_{i}: value_{i}")).collect();
        let content = entries.join("\n");
        let config = ChunkConfig { size: 200, overlap: 0 };
        let chunks = chunk_code(&content, "yaml", &config).unwrap();
        assert!(
            chunks.len() > 1,
            "large YAML should split into multiple chunks, got {}",
            chunks.len()
        );
    }

    #[test]
    fn markdown_large_section_splits_by_size() {
        // A single heading with enough content to exceed chunk_size
        let lines: Vec<String> = (0..50)
            .map(|i| format!("Paragraph line {i} with some filler text to add length."))
            .collect();
        let content = format!("# Big Section\n{}", lines.join("\n"));
        let config = ChunkConfig { size: 200, overlap: 50 };
        let chunks = chunk_markdown(&content, &config);
        assert!(
            chunks.len() > 1,
            "large section under one heading should still split by size, got {} chunks",
            chunks.len()
        );
        // First chunk should start with the heading
        assert!(chunks[0].starts_with("# Big Section"));
    }

    #[test]
    fn plain_chunk_overlap_zero_no_repeated_lines() {
        let content = (0..50).map(|i| format!("unique-line-{i}")).collect::<Vec<_>>().join("\n");
        let config = ChunkConfig { size: 200, overlap: 0 };
        let chunks = chunk_plain(&content, &config);
        assert!(chunks.len() > 1);

        // With zero overlap, no line should appear in more than one chunk
        let mut seen = std::collections::HashSet::new();
        for chunk in &chunks {
            for line in chunk.lines() {
                assert!(
                    seen.insert(line.to_string()),
                    "line {line:?} appeared in multiple chunks with overlap=0"
                );
            }
        }
    }

    #[test]
    fn chunk_code_tsx_with_jsx() {
        let content = r"
import React from 'react';

interface Props {
    name: string;
}

function Greeting({ name }: Props) {
    return <div>Hello, {name}!</div>;
}

export default Greeting;
";
        let chunks = chunk_code(content, "tsx", &ChunkConfig { size: 80, overlap: 10 }).unwrap();
        assert!(!chunks.is_empty());
        let joined = chunks.join("\n");
        assert!(joined.contains("Greeting"));
        assert!(joined.contains("Props"));
    }

    #[test]
    fn chunk_code_mjs_and_cjs_extensions() {
        let content = "export function hello() { return 42; }\n";
        let mjs_chunks = chunk_code(content, "mjs", &default_config()).unwrap();
        let cjs_chunks = chunk_code(content, "cjs", &default_config()).unwrap();
        let jsx_chunks = chunk_code(content, "jsx", &default_config()).unwrap();

        assert!(!mjs_chunks.is_empty());
        assert!(!cjs_chunks.is_empty());
        assert!(!jsx_chunks.is_empty());
    }

    #[test]
    fn chunk_code_mts_and_cts_extensions() {
        let content = "export function hello(): number { return 42; }\n";
        let mts_chunks = chunk_code(content, "mts", &default_config()).unwrap();
        let cts_chunks = chunk_code(content, "cts", &default_config()).unwrap();

        assert!(!mts_chunks.is_empty());
        assert!(!cts_chunks.is_empty());
    }
}
