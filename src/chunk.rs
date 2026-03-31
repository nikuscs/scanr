use anyhow::Result;

const CHUNK_SIZE: usize = 1000;
const CHUNK_OVERLAP: usize = 100;

pub fn chunk_code(source: &str, ext: &str) -> Result<Vec<String>> {
    let language = match ext {
        "ts" | "mts" | "cts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "js" | "mjs" | "cjs" | "jsx" => tree_sitter_javascript::LANGUAGE.into(),
        _ => return Ok(chunk_plain(source)),
    };

    chunk_with_tree_sitter(source, language)
}

pub fn chunk_markdown(source: &str) -> Vec<String> {
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

        if current.len() >= CHUNK_SIZE && !is_heading {
            chunks.push(current.trim().to_string());
            // Keep overlap from the end
            let overlap_start = current.len().saturating_sub(CHUNK_OVERLAP);
            let overlap = current[overlap_start..].to_string();
            current = overlap;
        }
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    chunks
}

fn chunk_with_tree_sitter(source: &str, language: tree_sitter::Language) -> Result<Vec<String>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language)?;

    let Some(tree) = parser.parse(source, None) else {
        return Ok(chunk_plain(source));
    };

    let root = tree.root_node();
    let mut chunks = Vec::new();
    let mut current = String::new();

    collect_nodes(root, source, &mut chunks, &mut current);

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    // If tree-sitter produced no useful chunks, fall back to plain splitting
    if chunks.is_empty() {
        return Ok(chunk_plain(source));
    }

    Ok(chunks)
}

fn collect_nodes(
    node: tree_sitter::Node,
    source: &str,
    chunks: &mut Vec<String>,
    current: &mut String,
) {
    // Top-level declarations become chunk boundaries
    if is_chunk_boundary(&node) {
        let text = &source[node.start_byte()..node.end_byte()];

        if !current.trim().is_empty() && current.len() + text.len() > CHUNK_SIZE {
            chunks.push(current.trim().to_string());
            // Keep overlap
            let overlap_start = current.len().saturating_sub(CHUNK_OVERLAP);
            *current = current[overlap_start..].to_string();
        }

        current.push_str(text);
        current.push('\n');

        if current.len() >= CHUNK_SIZE {
            chunks.push(current.trim().to_string());
            let overlap_start = current.len().saturating_sub(CHUNK_OVERLAP);
            *current = current[overlap_start..].to_string();
        }

        return;
    }

    // Recurse into children
    let child_count = node.child_count();
    if child_count == 0 {
        let text = &source[node.start_byte()..node.end_byte()];
        current.push_str(text);
        return;
    }

    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            collect_nodes(child, source, chunks, current);
        }
    }
}

fn is_chunk_boundary(node: &tree_sitter::Node) -> bool {
    // Only split on top-level or second-level declarations
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

    matches!(
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
    )
}

fn chunk_plain(source: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut start = 0;

    while start < lines.len() {
        let mut end = start;
        let mut len = 0;

        while end < lines.len() && len + lines[end].len() < CHUNK_SIZE {
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

        // Move back for overlap
        let overlap_lines = CHUNK_OVERLAP / 40; // rough average line length
        start = if end > overlap_lines { end - overlap_lines } else { end };
    }

    chunks
}
