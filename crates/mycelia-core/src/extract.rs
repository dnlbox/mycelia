use crate::model::EDGE_TYPE_CALLS;
use crate::{Chunk, EdgeDraft, SourceSpan};

const MAX_CHUNK_BYTES: usize = 2_048;
const EXTRACTOR_PLAIN: &str = "plain-text-v1";
const EXTRACTOR_RUST: &str = "tree-sitter-rust-v1";
const EXTRACTOR_TS: &str = "tree-sitter-typescript-v1";
const EXTRACTOR_TSX: &str = "tree-sitter-tsx-v1";
const EXTRACTOR_PY: &str = "tree-sitter-python-v1";
const EXTRACTOR_RUBY: &str = "tree-sitter-ruby-v1";
pub(crate) const EXTRACTOR_VERSIONS: &[&str] = &[
    EXTRACTOR_PLAIN,
    EXTRACTOR_RUST,
    EXTRACTOR_TS,
    EXTRACTOR_TSX,
    EXTRACTOR_PY,
    EXTRACTOR_RUBY,
];

const TOP_LEVEL_KINDS_RUST: &[&str] = &[
    "function_item",
    "impl_item",
    "struct_item",
    "enum_item",
    "type_item",
    "const_item",
    "trait_item",
    "static_item",
    "mod_item",
    "macro_definition",
];

const TOP_LEVEL_KINDS_TS: &[&str] = &[
    "function_declaration",
    "class_declaration",
    "interface_declaration",
    "type_alias_declaration",
    "enum_declaration",
    "abstract_class_declaration",
    "export_statement",
    "ambient_declaration",
    "module",
];

const TOP_LEVEL_KINDS_PY: &[&str] = &[
    "function_definition",
    "class_definition",
    "decorated_definition",
];

const TOP_LEVEL_KINDS_RUBY: &[&str] = &[
    "method",
    "singleton_method",
    "class",
    "module",
    "singleton_class",
];

/// Returns the extractor identifier that will be attempted for `source_path`.
/// This reflects intent (based on file extension), not which extractor succeeded.
pub(crate) fn extractor_id_for(source_path: &str) -> &'static str {
    if source_path.ends_with(".rs") {
        EXTRACTOR_RUST
    } else if source_path.ends_with(".ts") {
        EXTRACTOR_TS
    } else if source_path.ends_with(".tsx") {
        EXTRACTOR_TSX
    } else if source_path.ends_with(".py") {
        EXTRACTOR_PY
    } else if source_path.ends_with(".rb") {
        EXTRACTOR_RUBY
    } else {
        EXTRACTOR_PLAIN
    }
}

/// Returns `(chunks, fallback)` where `fallback` is `true` when a code file
/// could not be parsed by the structural extractor and plain-text was used.
pub(crate) fn extract_text(source_path: &str, source_hash: &str, text: &str) -> (Vec<Chunk>, bool) {
    if source_path.ends_with(".rs") {
        if let Some(chunks) = extract_rust(source_path, source_hash, text) {
            return (chunks, false);
        }
        return (extract_plain_text(source_path, source_hash, text), true);
    }
    if source_path.ends_with(".ts") {
        let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        if let Some(chunks) =
            extract_typescript(source_path, source_hash, text, language, EXTRACTOR_TS)
        {
            return (chunks, false);
        }
        return (extract_plain_text(source_path, source_hash, text), true);
    }
    if source_path.ends_with(".tsx") {
        let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();
        if let Some(chunks) =
            extract_typescript(source_path, source_hash, text, language, EXTRACTOR_TSX)
        {
            return (chunks, false);
        }
        return (extract_plain_text(source_path, source_hash, text), true);
    }
    if source_path.ends_with(".py") {
        if let Some(chunks) = extract_python(source_path, source_hash, text) {
            return (chunks, false);
        }
        return (extract_plain_text(source_path, source_hash, text), true);
    }
    if source_path.ends_with(".rb") {
        if let Some(chunks) = extract_ruby(source_path, source_hash, text) {
            return (chunks, false);
        }
        return (extract_plain_text(source_path, source_hash, text), true);
    }
    (extract_plain_text(source_path, source_hash, text), false)
}

fn extract_rust(source_path: &str, source_hash: &str, text: &str) -> Option<Vec<Chunk>> {
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;

    let tree = parser.parse(text, None)?;
    let root = tree.root_node();

    let mut walk = root.walk();
    let children: Vec<tree_sitter::Node<'_>> = root.named_children(&mut walk).collect();

    let line_index = LineIndex::new(text);
    let mut chunks = Vec::new();

    for (i, node) in children.iter().enumerate() {
        if node.is_error() || !TOP_LEVEL_KINDS_RUST.contains(&node.kind()) {
            continue;
        }

        // Walk backwards to collect contiguous outer doc comments (///).
        // Use tree-sitter row positions to detect blank lines reliably,
        // independent of whether the grammar includes trailing newlines in nodes.
        let mut doc_byte_start = node.start_byte();
        let mut next_row = node.start_position().row;
        let mut j = i;
        while j > 0 {
            j -= 1;
            let candidate = &children[j];
            if candidate.kind() != "line_comment" {
                break;
            }
            let comment_text = &text[candidate.start_byte()..candidate.end_byte()];
            if !comment_text
                .trim_start_matches([' ', '\t'])
                .starts_with("///")
            {
                break;
            }
            // The comment must be immediately before the next item — no blank line.
            // tree-sitter places end_position.row on the row AFTER the trailing \n,
            // so a comment on row L has end_row=L+1; the next item must start on that same row.
            let candidate_end_row = candidate.end_position().row;
            if next_row > candidate_end_row {
                break;
            }
            doc_byte_start = candidate.start_byte();
            next_row = candidate.start_position().row;
        }

        let byte_start = doc_byte_start;
        let byte_end = node.end_byte();

        let mut edges = Vec::new();
        collect_rust_calls(*node, text, &line_index, &mut edges);

        chunks.push(Chunk {
            id: chunk_id(source_path, source_hash, byte_start, byte_end),
            source_path: source_path.to_owned(),
            source_hash: source_hash.to_owned(),
            span: SourceSpan {
                byte_start,
                byte_end,
                line_start: line_index.line_at_start(byte_start),
                line_end: line_index.line_at_end(text, byte_end),
            },
            text: text[byte_start..byte_end].to_owned(),
            extractor: EXTRACTOR_RUST,
            symbol: rust_symbol_name(*node, text),
            edges,
        });
    }

    if chunks.is_empty() {
        return None;
    }
    Some(chunks)
}

/// The defined symbol name for a top-level Rust item. `impl` blocks have no
/// `name` field, so they are keyed by the bare type being implemented.
fn rust_symbol_name(node: tree_sitter::Node<'_>, text: &str) -> Option<String> {
    if node.kind() == "impl_item" {
        let type_node = node.child_by_field_name("type")?;
        let raw = &text[type_node.byte_range()];
        let bare = raw.split('<').next().unwrap_or(raw).trim();
        return (!bare.is_empty()).then(|| bare.to_owned());
    }
    let name = node.child_by_field_name("name")?;
    Some(text[name.byte_range()].to_owned())
}

/// Collects `calls` edges from one top-level Rust item by walking its subtree for
/// call and macro-invocation sites. Each edge is addressed by the callee's bare
/// name and carries the call-site span as provenance. Resolution to a defining
/// chunk is deferred to query time; names with no in-corpus definition (std
/// functions, external macros) simply resolve to nothing and are dropped there.
/// tree-sitter never parses identifiers inside comments or string literals as
/// calls, so this cannot manufacture an edge from prose.
fn collect_rust_calls(
    node: tree_sitter::Node<'_>,
    text: &str,
    line_index: &LineIndex,
    edges: &mut Vec<EdgeDraft>,
) {
    let callee = match node.kind() {
        "call_expression" => node.child_by_field_name("function"),
        "macro_invocation" => node.child_by_field_name("macro"),
        _ => None,
    };
    if let Some(callee) = callee
        && let Some((name, name_node)) = rust_callee_name(callee, text)
    {
        let byte_start = name_node.start_byte();
        let byte_end = name_node.end_byte();
        edges.push(EdgeDraft {
            edge_type: EDGE_TYPE_CALLS,
            dst_symbol: name,
            span: SourceSpan {
                byte_start,
                byte_end,
                line_start: line_index.line_at_start(byte_start),
                line_end: line_index.line_at_end(text, byte_end),
            },
        });
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_rust_calls(child, text, line_index, edges);
    }
}

/// Extracts the bare callee name and the identifier node to source it from, for
/// the `function`/`macro` position of a call. Only free-function calls
/// (`foo()`), path calls (`module::foo()`, final segment), and macro
/// invocations are captured. Method calls (`receiver.foo()`) are deliberately
/// dropped: the receiver type is unknown, so the bare method name would
/// misresolve to an unrelated free function of the same name (e.g. `row.get()`
/// to a `get` function). Per "a wrong connection is worse than none", a method
/// edge needs type resolution and is deferred. Resolving method calls is a
/// follow-up that needs type information.
fn rust_callee_name<'tree>(
    func: tree_sitter::Node<'tree>,
    text: &str,
) -> Option<(String, tree_sitter::Node<'tree>)> {
    match func.kind() {
        "identifier" => Some((text[func.byte_range()].to_owned(), func)),
        "scoped_identifier" => {
            let name = func.child_by_field_name("name")?;
            Some((text[name.byte_range()].to_owned(), name))
        }
        "generic_function" => {
            let inner = func.child_by_field_name("function")?;
            rust_callee_name(inner, text)
        }
        _ => None,
    }
}

fn extract_typescript(
    source_path: &str,
    source_hash: &str,
    text: &str,
    language: tree_sitter::Language,
    extractor: &'static str,
) -> Option<Vec<Chunk>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;

    let tree = parser.parse(text, None)?;
    let root = tree.root_node();

    let mut walk = root.walk();
    let children: Vec<tree_sitter::Node<'_>> = root.named_children(&mut walk).collect();

    let line_index = LineIndex::new(text);
    let mut chunks = Vec::new();

    for (i, node) in children.iter().enumerate() {
        if node.is_error() || !TOP_LEVEL_KINDS_TS.contains(&node.kind()) {
            continue;
        }

        // Collect a leading JSDoc block comment (`/** ... */`) if one immediately
        // precedes this declaration with no blank line between them.
        let mut doc_byte_start = node.start_byte();
        let next_row = node.start_position().row;
        if i > 0 {
            let candidate = &children[i - 1];
            if candidate.kind() == "comment" {
                let comment_text = &text[candidate.start_byte()..candidate.end_byte()];
                if comment_text
                    .trim_start_matches([' ', '\t'])
                    .starts_with("/**")
                {
                    let candidate_end_row = candidate.end_position().row;
                    if next_row <= candidate_end_row + 1 {
                        doc_byte_start = candidate.start_byte();
                    }
                }
            }
        }

        let byte_start = doc_byte_start;
        let byte_end = node.end_byte();

        chunks.push(Chunk {
            id: chunk_id(source_path, source_hash, byte_start, byte_end),
            source_path: source_path.to_owned(),
            source_hash: source_hash.to_owned(),
            span: SourceSpan {
                byte_start,
                byte_end,
                line_start: line_index.line_at_start(byte_start),
                line_end: line_index.line_at_end(text, byte_end),
            },
            text: text[byte_start..byte_end].to_owned(),
            extractor,
            symbol: None,
            edges: Vec::new(),
        });
    }

    if chunks.is_empty() {
        return None;
    }
    Some(chunks)
}

fn extract_python(source_path: &str, source_hash: &str, text: &str) -> Option<Vec<Chunk>> {
    let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;

    let tree = parser.parse(text, None)?;
    let root = tree.root_node();

    let mut walk = root.walk();
    let children: Vec<tree_sitter::Node<'_>> = root.named_children(&mut walk).collect();

    let line_index = LineIndex::new(text);
    let mut chunks = Vec::new();

    for node in &children {
        if node.is_error() || !TOP_LEVEL_KINDS_PY.contains(&node.kind()) {
            continue;
        }

        let byte_start = node.start_byte();
        let byte_end = node.end_byte();

        chunks.push(Chunk {
            id: chunk_id(source_path, source_hash, byte_start, byte_end),
            source_path: source_path.to_owned(),
            source_hash: source_hash.to_owned(),
            span: SourceSpan {
                byte_start,
                byte_end,
                line_start: line_index.line_at_start(byte_start),
                line_end: line_index.line_at_end(text, byte_end),
            },
            text: text[byte_start..byte_end].to_owned(),
            extractor: EXTRACTOR_PY,
            symbol: None,
            edges: Vec::new(),
        });
    }

    if chunks.is_empty() {
        return None;
    }
    Some(chunks)
}

fn extract_ruby(source_path: &str, source_hash: &str, text: &str) -> Option<Vec<Chunk>> {
    let language: tree_sitter::Language = tree_sitter_ruby::LANGUAGE.into();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;

    let tree = parser.parse(text, None)?;
    let root = tree.root_node();

    let mut walk = root.walk();
    let children: Vec<tree_sitter::Node<'_>> = root.named_children(&mut walk).collect();

    let line_index = LineIndex::new(text);
    let mut chunks = Vec::new();

    for (i, node) in children.iter().enumerate() {
        if node.is_error() || !TOP_LEVEL_KINDS_RUBY.contains(&node.kind()) {
            continue;
        }

        // Walk backwards to collect contiguous leading `#` comment lines.
        let mut doc_byte_start = node.start_byte();
        let mut next_row = node.start_position().row;
        let mut j = i;
        while j > 0 {
            j -= 1;
            let candidate = &children[j];
            if candidate.kind() != "comment" {
                break;
            }
            // The comment must be immediately before the next item — no blank line.
            // Ruby comment nodes do not include the trailing newline, so a comment
            // on row L has end_row=L; the next item is adjacent when it starts on L+1.
            let candidate_end_row = candidate.end_position().row;
            if next_row > candidate_end_row + 1 {
                break;
            }
            doc_byte_start = candidate.start_byte();
            next_row = candidate.start_position().row;
        }

        let byte_start = doc_byte_start;
        let byte_end = node.end_byte();

        chunks.push(Chunk {
            id: chunk_id(source_path, source_hash, byte_start, byte_end),
            source_path: source_path.to_owned(),
            source_hash: source_hash.to_owned(),
            span: SourceSpan {
                byte_start,
                byte_end,
                line_start: line_index.line_at_start(byte_start),
                line_end: line_index.line_at_end(text, byte_end),
            },
            text: text[byte_start..byte_end].to_owned(),
            extractor: EXTRACTOR_RUBY,
            symbol: None,
            edges: Vec::new(),
        });
    }

    if chunks.is_empty() {
        return None;
    }
    Some(chunks)
}

fn extract_plain_text(source_path: &str, source_hash: &str, text: &str) -> Vec<Chunk> {
    let line_index = LineIndex::new(text);
    paragraph_ranges(text)
        .into_iter()
        .flat_map(|range| split_range(text, range))
        .map(|(byte_start, byte_end)| Chunk {
            id: chunk_id(source_path, source_hash, byte_start, byte_end),
            source_path: source_path.to_owned(),
            source_hash: source_hash.to_owned(),
            span: SourceSpan {
                byte_start,
                byte_end,
                line_start: line_index.line_at_start(byte_start),
                line_end: line_index.line_at_end(text, byte_end),
            },
            text: text[byte_start..byte_end].to_owned(),
            extractor: EXTRACTOR_PLAIN,
            symbol: None,
            edges: Vec::new(),
        })
        .collect()
}

fn paragraph_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut paragraph_start = None;
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let end = offset + line.len();
        if line.trim().is_empty() {
            if let Some(start) = paragraph_start.take() {
                ranges.push((start, offset));
            }
        } else {
            paragraph_start.get_or_insert(offset);
        }
        offset = end;
    }

    if offset < text.len() {
        paragraph_start.get_or_insert(offset);
        offset = text.len();
    }

    if let Some(start) = paragraph_start {
        ranges.push((start, offset));
    }

    ranges
        .into_iter()
        .filter(|(start, end)| start < end && !text[*start..*end].trim().is_empty())
        .collect()
}

fn split_range(text: &str, (start, end): (usize, usize)) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut cursor = start;

    while end - cursor > MAX_CHUNK_BYTES {
        let hard_end = previous_char_boundary(text, cursor + MAX_CHUNK_BYTES);
        let candidate = &text[cursor..hard_end];
        let preferred = candidate
            .char_indices()
            .rev()
            .find(|(_, character)| character.is_whitespace())
            .map(|(index, character)| cursor + index + character.len_utf8())
            .filter(|split| *split > cursor);
        let split = preferred.unwrap_or(hard_end);
        ranges.push((cursor, split));
        cursor = split;
    }

    if cursor < end {
        ranges.push((cursor, end));
    }

    ranges
}

fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Precomputed newline byte offsets for one file. Building this once lets each
/// chunk's line numbers cost O(log n) via binary search instead of rescanning
/// the document from the start, which mattered on large single-file corpora
/// where many chunks share one source.
struct LineIndex {
    /// Ascending byte offset of every `\n` in the source.
    newlines: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let newlines = text
            .bytes()
            .enumerate()
            .filter_map(|(index, byte)| (byte == b'\n').then_some(index))
            .collect();
        Self { newlines }
    }

    /// Count of newlines strictly before `byte`. The offsets are sorted, so the
    /// `offset < byte` predicate partitions the slice.
    fn newlines_before(&self, byte: usize) -> usize {
        self.newlines.partition_point(|&offset| offset < byte)
    }

    /// 1-based line containing `byte_start`.
    fn line_at_start(&self, byte_start: usize) -> usize {
        self.newlines_before(byte_start) + 1
    }

    /// 1-based line for an exclusive `byte_end`: a span that ends exactly on a
    /// newline keeps the line it closed rather than spilling onto the next.
    fn line_at_end(&self, text: &str, byte_end: usize) -> usize {
        let newline_count = self.newlines_before(byte_end);
        if byte_end > 0 && text.as_bytes()[byte_end - 1] == b'\n' {
            newline_count.max(1)
        } else {
            newline_count + 1
        }
    }
}

fn chunk_id(source_path: &str, source_hash: &str, byte_start: usize, byte_end: usize) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(source_path.as_bytes());
    hasher.update(&[0]);
    hasher.update(source_hash.as_bytes());
    hasher.update(&[0]);
    hasher.update(byte_start.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(byte_end.to_string().as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- line index ---

    #[test]
    fn line_index_matches_full_scan_including_boundaries() {
        let text = "alpha\nbeta\n\ngamma";
        let index = LineIndex::new(text);

        // Naive O(n) scan that the binary-searched index must agree with.
        let scan_start = |byte: usize| text[..byte].bytes().filter(|b| *b == b'\n').count() + 1;

        for byte in 0..=text.len() {
            if !text.is_char_boundary(byte) {
                continue;
            }
            assert_eq!(
                index.line_at_start(byte),
                scan_start(byte),
                "start at {byte}"
            );
        }

        // A span ending exactly on a newline keeps the line it closed.
        assert_eq!(index.line_at_end(text, 6), 1); // "alpha\n"
        assert_eq!(index.line_at_end(text, 11), 2); // through "beta\n"
        assert_eq!(index.line_at_end(text, text.len()), 4); // "gamma" after blank line 3
        assert_eq!(index.line_at_end(text, 0), 1); // empty prefix
    }

    // --- plain-text tests ---

    #[test]
    fn extracts_paragraphs_with_exact_spans() {
        let text = "first line\nsecond line\n\nthird paragraph";
        let (chunks, fallback) = extract_text("notes.txt", "hash", text);

        assert!(!fallback);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "first line\nsecond line\n");
        assert_eq!(
            chunks[0].span,
            SourceSpan {
                byte_start: 0,
                byte_end: 23,
                line_start: 1,
                line_end: 2,
            }
        );
        assert_eq!(chunks[1].text, "third paragraph");
        assert_eq!(chunks[1].span.line_start, 4);
        assert_eq!(chunks[1].span.line_end, 4);
    }

    #[test]
    fn preserves_utf8_boundaries_when_splitting() {
        let text = "é".repeat(1_200);
        let (chunks, fallback) = extract_text("unicode.txt", "hash", &text);

        assert!(!fallback);
        assert_eq!(chunks.len(), 2);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.text.len() <= MAX_CHUNK_BYTES)
        );
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.text.as_str())
                .collect::<String>(),
            text
        );
        assert!(
            chunks
                .iter()
                .all(|chunk| text.is_char_boundary(chunk.span.byte_start)
                    && text.is_char_boundary(chunk.span.byte_end))
        );
    }

    #[test]
    fn prefers_whitespace_for_oversized_paragraphs() {
        let text = format!("{} {}", "a".repeat(1_900), "b".repeat(300));
        let (chunks, _) = extract_text("large.txt", "hash", &text);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text.len(), 1_901);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.text.len() <= MAX_CHUNK_BYTES)
        );
    }

    #[test]
    fn ids_are_stable_and_sensitive_to_identity() {
        let (first, _) = extract_text("a.txt", "hash", "content");
        let (same, _) = extract_text("a.txt", "hash", "content");
        let (other_path, _) = extract_text("b.txt", "hash", "content");
        let (other_hash, _) = extract_text("a.txt", "other", "content");

        assert_eq!(first[0].id, same[0].id);
        assert_ne!(first[0].id, other_path[0].id);
        assert_ne!(first[0].id, other_hash[0].id);
    }

    #[test]
    fn ignores_empty_paragraphs() {
        let (chunks, _) = extract_text("empty.txt", "hash", "\n \n\t\n");
        assert!(chunks.is_empty());
    }

    // --- tree-sitter Rust extractor tests ---

    #[test]
    fn extracts_top_level_rust_functions() {
        let src = r#"fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn subtract(a: i32, b: i32) -> i32 {
    a - b
}
"#;
        let (chunks, fallback) = extract_text("lib.rs", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("fn add"));
        assert!(chunks[1].text.contains("fn subtract"));
        assert_eq!(chunks[0].extractor, EXTRACTOR_RUST);
        assert_eq!(chunks[0].span.line_start, 1);
    }

    #[test]
    fn includes_leading_doc_comment_in_chunk() {
        let src = r#"/// Adds two numbers.
/// Returns their sum.
fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let (chunks, fallback) = extract_text("lib.rs", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("/// Adds two numbers."));
        assert!(chunks[0].text.contains("fn add"));
        assert_eq!(chunks[0].span.line_start, 1);
    }

    #[test]
    fn excludes_non_doc_comment_from_chunk() {
        let src = r#"// regular comment, not a doc comment
fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let (chunks, _) = extract_text("lib.rs", "hash", src);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("fn add"));
    }

    #[test]
    fn doc_comment_separated_by_blank_line_not_included() {
        let src = r#"/// This doc comment is separated by a blank line.

fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let (chunks, _) = extract_text("lib.rs", "hash", src);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("fn add"));
    }

    #[test]
    fn extracts_struct_enum_impl_trait() {
        let src = r#"struct Point { x: f64, y: f64 }

enum Color { Red, Green, Blue }

impl Point {
    fn origin() -> Self { Point { x: 0.0, y: 0.0 } }
}

trait Shape {
    fn area(&self) -> f64;
}
"#;
        let (chunks, fallback) = extract_text("shapes.rs", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 4);
        let kinds: Vec<&str> = chunks
            .iter()
            .map(|c| {
                if c.text.starts_with("struct") {
                    "struct"
                } else if c.text.starts_with("enum") {
                    "enum"
                } else if c.text.starts_with("impl") {
                    "impl"
                } else {
                    "trait"
                }
            })
            .collect();
        assert_eq!(kinds, ["struct", "enum", "impl", "trait"]);
    }

    #[test]
    fn rust_chunk_ids_are_deterministic() {
        let src = "fn foo() {}\n";
        let (first, _) = extract_text("a.rs", "hash", src);
        let (second, _) = extract_text("a.rs", "hash", src);
        assert_eq!(first[0].id, second[0].id);
    }

    // --- Rust symbol identity and `calls` edges ---

    /// Collects every `calls` edge target across the chunks, for assertions.
    fn call_targets(chunks: &[Chunk]) -> Vec<&str> {
        chunks
            .iter()
            .flat_map(|chunk| chunk.edges.iter())
            .map(|edge| edge.dst_symbol.as_str())
            .collect()
    }

    #[test]
    fn rust_chunks_carry_their_symbol_name() {
        let src = r#"fn add(a: i32, b: i32) -> i32 {
    a + b
}

struct Point { x: f64 }

impl Point {
    fn origin() -> Self { Point { x: 0.0 } }
}
"#;
        let (chunks, _) = extract_text("lib.rs", "hash", src);
        let symbols: Vec<Option<&str>> = chunks.iter().map(|c| c.symbol.as_deref()).collect();
        assert_eq!(symbols, vec![Some("add"), Some("Point"), Some("Point")]);
    }

    #[test]
    fn rust_collects_free_function_call_edges() {
        let src = r#"fn helper() -> i32 {
    42
}

fn caller() -> i32 {
    helper() + helper()
}
"#;
        let (chunks, _) = extract_text("lib.rs", "hash", src);
        // `caller` calls `helper` twice; one edge per call site.
        let caller = chunks
            .iter()
            .find(|c| c.symbol.as_deref() == Some("caller"))
            .unwrap();
        let targets: Vec<&str> = caller.edges.iter().map(|e| e.dst_symbol.as_str()).collect();
        assert_eq!(targets, vec!["helper", "helper"]);
        // Each edge is sourced at the call site, not the whole function.
        assert!(
            caller
                .edges
                .iter()
                .all(|e| e.span.byte_start >= caller.span.byte_start)
        );
        assert_eq!(caller.edges[0].edge_type, EDGE_TYPE_CALLS);
    }

    #[test]
    fn rust_path_call_uses_final_segment() {
        let src = r#"fn run() {
    module::do_work();
    Type::associated();
}
"#;
        let (chunks, _) = extract_text("lib.rs", "hash", src);
        assert_eq!(call_targets(&chunks), vec!["do_work", "associated"]);
    }

    #[test]
    fn rust_method_call_is_not_an_edge() {
        // Method calls are dropped: the receiver type is unknown, so a bare
        // method name would misresolve to an unrelated free function. `helper`
        // (a free call) is still captured; `process` (a method) is not.
        let src = r#"fn run(value: Thing) {
    helper();
    value.process();
}
"#;
        let (chunks, _) = extract_text("lib.rs", "hash", src);
        assert_eq!(call_targets(&chunks), vec!["helper"]);
    }

    #[test]
    fn rust_macro_invocation_is_a_call_edge() {
        let src = r#"fn run() {
    log_event!();
}
"#;
        let (chunks, _) = extract_text("lib.rs", "hash", src);
        assert_eq!(call_targets(&chunks), vec!["log_event"]);
    }

    #[test]
    fn rust_no_false_edge_from_comment_or_string() {
        let src = r#"fn run() {
    // ghost() is only a comment
    let _ = "phantom()";
}
"#;
        let (chunks, _) = extract_text("lib.rs", "hash", src);
        assert!(call_targets(&chunks).is_empty());
    }

    #[test]
    fn rust_extractor_id_is_correct() {
        assert_eq!(extractor_id_for("src/lib.rs"), EXTRACTOR_RUST);
        assert_eq!(extractor_id_for("docs/README.md"), EXTRACTOR_PLAIN);
        assert_eq!(extractor_id_for("notes.txt"), EXTRACTOR_PLAIN);
    }

    #[test]
    fn plain_text_fallback_for_non_rs_files() {
        let text = "Some prose content\n\nSecond paragraph";
        let (chunks, fallback) = extract_text("docs/design.md", "hash", text);
        assert!(!fallback);
        assert_eq!(chunks[0].extractor, EXTRACTOR_PLAIN);
    }

    // --- tree-sitter TypeScript extractor tests ---

    #[test]
    fn extractor_id_for_typescript_and_python() {
        assert_eq!(extractor_id_for("src/index.ts"), EXTRACTOR_TS);
        assert_eq!(extractor_id_for("src/App.tsx"), EXTRACTOR_TSX);
        assert_eq!(extractor_id_for("agents/brain.py"), EXTRACTOR_PY);
    }

    #[test]
    fn extracts_top_level_typescript_functions() {
        let src = r#"function greet(name: string): string {
  return `Hello, ${name}!`;
}

async function fetchData(url: string): Promise<Response> {
  return fetch(url);
}
"#;
        let (chunks, fallback) = extract_text("utils.ts", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("function greet"));
        assert!(chunks[1].text.contains("async function fetchData"));
        assert_eq!(chunks[0].extractor, EXTRACTOR_TS);
    }

    #[test]
    fn extracts_exported_typescript_declarations() {
        let src = r#"export interface RunStore {
  save(run: Run): Promise<void>;
  load(id: string): Promise<Run | null>;
}

export type Status = "pending" | "running" | "done";

export class Orchestrator {
  async advance(run: Run): Promise<Run> {
    return run;
  }
}
"#;
        let (chunks, fallback) = extract_text("store.ts", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].text.contains("interface RunStore"));
        assert!(chunks[1].text.contains("type Status"));
        assert!(chunks[2].text.contains("class Orchestrator"));
        assert_eq!(chunks[0].extractor, EXTRACTOR_TS);
    }

    #[test]
    fn typescript_includes_leading_jsdoc_comment() {
        let src = r#"/**
 * Advances a run through the pipeline.
 * Idempotent at gates and terminal states.
 */
export async function advance(run: Run): Promise<Run> {
  return run;
}
"#;
        let (chunks, fallback) = extract_text("orchestrator.ts", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("/**"));
        assert!(chunks[0].text.contains("Advances a run"));
        assert!(chunks[0].text.contains("export async function advance"));
        assert_eq!(chunks[0].span.line_start, 1);
    }

    #[test]
    fn typescript_jsdoc_separated_by_blank_line_not_included() {
        let src = r#"/**
 * This JSDoc block is separated by a blank line.
 */

export function orphan(): void {}
"#;
        let (chunks, _) = extract_text("orphan.ts", "hash", src);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("export function orphan"));
    }

    // --- tree-sitter Python extractor tests ---

    #[test]
    fn extracts_top_level_python_functions_and_classes() {
        let src = r#"def compute_score(query: str, doc: str) -> float:
    """Return BM25 similarity between query and doc."""
    return 0.0


class BrainAgent:
    def __init__(self, model: str) -> None:
        self.model = model

    def respond(self, prompt: str) -> str:
        return ""
"#;
        let (chunks, fallback) = extract_text("brain.py", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("def compute_score"));
        assert!(chunks[0].text.contains("BM25 similarity"));
        assert!(chunks[1].text.contains("class BrainAgent"));
        assert_eq!(chunks[0].extractor, EXTRACTOR_PY);
    }

    #[test]
    fn python_decorated_definition_is_one_chunk() {
        let src = r#"@router.post("/runs")
async def create_run(request: Request) -> Response:
    """Create a new pipeline run."""
    return Response()
"#;
        let (chunks, fallback) = extract_text("routes.py", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("@router.post"));
        assert!(chunks[0].text.contains("async def create_run"));
    }

    // --- tree-sitter Ruby extractor tests ---

    #[test]
    fn extractor_id_for_ruby() {
        assert_eq!(extractor_id_for("app/models/user.rb"), EXTRACTOR_RUBY);
        assert_eq!(extractor_id_for("lib/runner.rb"), EXTRACTOR_RUBY);
    }

    #[test]
    fn extracts_top_level_ruby_methods_and_classes() {
        let src = r#"def greet(name)
  "Hello, #{name}!"
end

class User
  def initialize(name)
    @name = name
  end
end
"#;
        let (chunks, fallback) = extract_text("app.rb", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("def greet"));
        assert!(chunks[1].text.contains("class User"));
        assert_eq!(chunks[0].extractor, EXTRACTOR_RUBY);
    }

    #[test]
    fn ruby_extracts_module() {
        let src = r#"module Payments
  def self.charge(amount)
    # ...
  end
end
"#;
        let (chunks, fallback) = extract_text("payments.rb", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("module Payments"));
    }

    #[test]
    fn ruby_extracts_singleton_method() {
        let src = r#"def self.find(id)
  DB.query(id)
end
"#;
        let (chunks, fallback) = extract_text("record.rb", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("def self.find"));
    }

    #[test]
    fn ruby_includes_leading_comment_block() {
        let src = r#"# Returns a greeting string.
# Accepts any name.
def greet(name)
  "Hello, #{name}!"
end
"#;
        let (chunks, fallback) = extract_text("greeter.rb", "hash", src);
        assert!(!fallback);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("# Returns a greeting string."));
        assert!(chunks[0].text.contains("def greet"));
        assert_eq!(chunks[0].span.line_start, 1);
    }

    #[test]
    fn ruby_comment_separated_by_blank_line_not_included() {
        let src = r#"# This comment has a blank line below it.

def greet(name)
  "Hello, #{name}!"
end
"#;
        let (chunks, _) = extract_text("greeter.rb", "hash", src);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("def greet"));
    }

    #[test]
    fn ruby_chunk_ids_are_deterministic() {
        let src = "def foo\n  42\nend\n";
        let (first, _) = extract_text("a.rb", "hash", src);
        let (second, _) = extract_text("a.rb", "hash", src);
        assert_eq!(first[0].id, second[0].id);
    }
}
