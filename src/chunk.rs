use anyhow::Result;

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
            let overlap_start = current.len().saturating_sub(config.overlap);
            let overlap = current[overlap_start..].to_string();
            current = overlap;
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
            let overlap_start = current.len().saturating_sub(config.overlap);
            *current = current[overlap_start..].to_string();
        }

        current.push_str(text);
        current.push('\n');

        if current.len() >= config.size {
            chunks.push(current.trim().to_string());
            let overlap_start = current.len().saturating_sub(config.overlap);
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

    for i in 0..child_count {
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

fn chunk_plain(source: &str, config: &ChunkConfig) -> Vec<String> {
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
        start = if end > overlap_lines { end - overlap_lines } else { end };
    }

    chunks
}
