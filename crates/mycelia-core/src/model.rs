use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceSpan {
    pub byte_start: usize,
    pub byte_end: usize,
    pub line_start: usize,
    pub line_end: usize,
}

/// The edge type for a `calls` relationship. Edge types are stored as text in
/// the `edges` table; this mirrors the `&'static str` extractor convention so a
/// new edge type is a constant, not an enum migration.
pub const EDGE_TYPE_CALLS: &str = "calls";

/// Extraction-time confidence for a deterministic tree-sitter edge. Query-time
/// ambiguity (a callee name resolving to several definitions) is computed at
/// resolution, not stored.
pub const EDGE_CONFIDENCE_EXTRACTED: &str = "EXTRACTED";

/// A typed edge collected during extraction, addressed by the callee's bare
/// name (`dst_symbol`). Resolution to a defining chunk happens at query time
/// against the current symbol index. `span` is the call site inside the owning
/// chunk, so the relationship is sourced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeDraft {
    pub edge_type: &'static str,
    pub dst_symbol: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chunk {
    pub id: String,
    pub source_path: String,
    pub source_hash: String,
    pub span: SourceSpan,
    pub text: String,
    pub extractor: &'static str,
    /// The defined symbol name for a code chunk; `None` for plain text and for
    /// languages without edge extraction yet.
    pub symbol: Option<String>,
    /// Typed edges originating in this chunk (currently Rust `calls` only).
    pub edges: Vec<EdgeDraft>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChunkRecord {
    pub id: String,
    pub source_path: String,
    pub source_hash: String,
    pub span: SourceSpan,
    pub text: String,
    pub extractor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Outcome of a freshness-validated `retrieve`. Precision over caching: the
/// caller is only ever handed content the index is sure is real and current.
/// When the source still matches what was indexed, the precise chunk is
/// returned; when it has changed, the whole current file is read live and
/// returned instead, so the answer is always up to date and never a stale chunk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Retrieved {
    /// Source unchanged since indexing; the precise indexed chunk is current.
    Ok {
        #[serde(flatten)]
        chunk: ChunkRecord,
    },
    /// Source changed since indexing; the indexed chunk is no longer trustworthy,
    /// so the full current file is read live and returned. The caller gets real,
    /// up-to-date code rather than a stale slice.
    File {
        source_path: String,
        line_start: usize,
        line_end: usize,
        text: String,
    },
    /// No trustworthy content to return: the source was removed, is unreadable,
    /// or is no longer UTF-8 text. The caller gets a signal, never stale bytes.
    Unavailable {
        chunk_id: String,
        source_path: String,
        message: String,
    },
}

impl Retrieved {
    /// The wire `status` tag for this outcome (`ok`, `file`, or `unavailable`).
    pub fn status(&self) -> &'static str {
        match self {
            Self::Ok { .. } => "ok",
            Self::File { .. } => "file",
            Self::Unavailable { .. } => "unavailable",
        }
    }

    /// The whole current file, returned when the indexed chunk went stale. Line
    /// range spans the live file so provenance stays accurate.
    pub(crate) fn file(source_path: String, text: String) -> Self {
        let line_end = text.lines().count().max(1);
        Self::File {
            source_path,
            line_start: 1,
            line_end,
            text,
        }
    }

    pub(crate) fn unavailable(record: &ChunkRecord, reason: &str) -> Self {
        Self::Unavailable {
            chunk_id: record.id.clone(),
            source_path: record.source_path.clone(),
            message: format!("{reason}; re-run find or `mycelia refresh`"),
        }
    }
}

/// Result of self-healing one source file in the bound index. Reported so a
/// caller can tell whether drift was corrected, the source was dropped, or
/// nothing needed doing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SourceRefresh {
    /// The changed file was re-chunked and its chunks replaced.
    Reindexed { chunks: usize },
    /// The file was gone or no longer indexable text; its chunks were removed.
    Pruned,
    /// The file still matched the index; nothing changed.
    Unchanged,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalStrategy {
    Substring,
    Fts5,
    #[default]
    Fts5Reranked,
    Vector,
    Hybrid,
    Routed,
}

impl std::fmt::Display for RetrievalStrategy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Substring => formatter.write_str("substring"),
            Self::Fts5 => formatter.write_str("fts5"),
            Self::Fts5Reranked => formatter.write_str("fts5_reranked"),
            Self::Vector => formatter.write_str("vector"),
            Self::Hybrid => formatter.write_str("hybrid"),
            Self::Routed => formatter.write_str("routed"),
        }
    }
}

/// Direction of a relationship query over `calls` edges. `Callers` finds the
/// chunks that call the queried symbol; `Callees` finds the chunks the queried
/// symbol calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Callers,
    Callees,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Callers => "callers",
            Self::Callees => "callees",
        }
    }
}

/// One sourced relationship. `definition` is the related chunk (the caller for a
/// callers query, the callee's definition for a callees query); `call_site` is
/// where the call appears, so the relationship is verifiable. `resolved` is true
/// only when the name maps to exactly one in-corpus definition: per the north
/// star "a wrong connection is worse than none", an ambiguous name is surfaced
/// with every candidate and `resolved = false`, never silently collapsed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RelatedHit {
    pub symbol: String,
    pub definition: SearchHeader,
    pub call_site: SourceSpan,
    pub resolved: bool,
    pub definition_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchHit {
    #[serde(flatten)]
    pub chunk: ChunkRecord,
    pub score: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchHeader {
    pub chunk_id: String,
    pub source_path: String,
    pub source_hash: String,
    pub span: SourceSpan,
    pub extractor: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub synopsis: String,
}

impl SearchHeader {
    pub fn from_hit(hit: &SearchHit) -> Self {
        Self::from_record(&hit.chunk, hit.score)
    }

    /// Distil a header from a record directly, for paths that carry no ranking
    /// score (such as graph relationship results).
    pub fn from_record(chunk: &ChunkRecord, score: f64) -> Self {
        let (signature, synopsis) = distill_header(chunk);
        Self {
            chunk_id: chunk.id.clone(),
            source_path: chunk.source_path.clone(),
            source_hash: chunk.source_hash.clone(),
            span: chunk.span.clone(),
            extractor: chunk.extractor.clone(),
            score,
            signature,
            synopsis,
        }
    }

    pub fn approximate_bytes(&self) -> usize {
        self.chunk_id.len()
            + self.source_path.len()
            + self.source_hash.len()
            + self.extractor.len()
            + self.signature.as_ref().map_or(0, String::len)
            + self.synopsis.len()
            + 160
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct IndexReport {
    pub discovered: usize,
    pub indexed: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub rejected: usize,
    pub chunks_written: usize,
    pub code_parse_fallbacks: usize,
    pub elapsed_ms: u128,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EmbeddingReport {
    pub model_id: String,
    pub dimensions: usize,
    pub embedded: usize,
    pub unchanged: usize,
    pub removed_other_models: usize,
    pub storage_bytes: usize,
    pub elapsed_ms: u128,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ExpectedMatch {
    pub source_path: String,
    #[serde(default)]
    pub contains: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct EvaluationCase {
    pub name: String,
    pub query: String,
    #[serde(default)]
    pub required_files: Vec<String>,
    #[serde(default)]
    pub expected: Vec<ExpectedMatch>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EvaluationCaseResult {
    pub name: String,
    pub query: String,
    pub required_files: Vec<String>,
    pub rank: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EvaluationReport {
    pub strategy: RetrievalStrategy,
    pub limit: usize,
    pub cases: usize,
    pub hits: usize,
    pub hit_rate: f64,
    pub mean_reciprocal_rank: f64,
    pub elapsed_ms: u128,
    pub token_usage: TokenUsageReport,
    pub results: Vec<EvaluationCaseResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BaselineEvaluationReport {
    pub name: String,
    pub limit: usize,
    pub cases: usize,
    pub hits: usize,
    pub hit_rate: f64,
    pub mean_reciprocal_rank: f64,
    pub elapsed_ms: u128,
    pub token_usage: TokenUsageReport,
    pub results: Vec<EvaluationCaseResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PairedEvaluationReport {
    pub mycelia: EvaluationReport,
    pub baseline: BaselineEvaluationReport,
    pub comparison: EvaluationComparison,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EvaluationComparison {
    pub hit_rate_delta: f64,
    pub mean_reciprocal_rank_delta: f64,
    pub tokens_per_answer_delta: f64,
    pub token_reduction_ratio: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TokenUsageReport {
    pub answered_queries: usize,
    pub find_header_tokens: usize,
    pub retrieved_body_tokens: usize,
    pub answer_tokens: usize,
    pub tokens_per_answer: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cold_source_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cold_tokens_per_answer: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct CorpusStatusReport {
    pub chunk_count: usize,
    pub embedding_count: usize,
    pub embedding_model: Option<String>,
    pub db_size_bytes: u64,
    /// Chunks carrying a defined symbol name. Zero on a corpus indexed before the
    /// graph migration, signalling that a `refresh` is needed to populate it.
    pub symbol_count: usize,
    /// Typed `calls` edges stored across the corpus.
    pub edge_count: usize,
}

fn distill_header(chunk: &ChunkRecord) -> (Option<String>, String) {
    if chunk.extractor.starts_with("tree-sitter-") {
        let signature = distill_code_signature(chunk.text.as_str());
        let synopsis = doc_synopsis(chunk.extractor.as_str(), chunk.text.as_str())
            .unwrap_or_else(|| signature.clone());
        return (Some(signature), synopsis);
    }

    let synopsis = chunk
        .text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_owned();
    (None, trim_to_header(synopsis.as_str()))
}

/// True for lines that precede or decorate a declaration but are not part of
/// its signature: comments (line and block, including JSDoc continuations),
/// Rust attributes, and Python docstring fences.
fn is_signature_noise(line: &str) -> bool {
    line.is_empty()
        || line.starts_with("//")
        || line.starts_with("#[")
        || line.starts_with("/*")
        || line.starts_with('*')
        || line.starts_with("\"\"\"")
        || line.starts_with("'''")
}

fn distill_code_signature(text: &str) -> String {
    let mut parts = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if is_signature_noise(trimmed) {
            continue;
        }
        parts.push(trimmed);
        // Stop at the end of the declaration line: a block opener, a statement
        // terminator, an assignment, or a Python `def`/`class` colon.
        if trimmed.contains('{')
            || trimmed.ends_with(';')
            || trimmed.contains(" = ")
            || trimmed.ends_with(':')
        {
            break;
        }
        if parts.len() == 4 {
            break;
        }
    }

    let joined = parts.join(" ");
    let mut end = joined.len();
    for delimiter in ['{', '='] {
        if let Some(index) = joined.find(delimiter) {
            end = end.min(index);
        }
    }
    trim_to_header(joined[..end].trim())
}

/// Extracts the first human-readable documentation line for a code chunk,
/// keyed to the extractor's comment convention.
fn doc_synopsis(extractor: &str, text: &str) -> Option<String> {
    if extractor.contains("rust") {
        return first_rust_doc_line(text);
    }
    if extractor.contains("typescript") || extractor.contains("tsx") {
        return first_jsdoc_line(text);
    }
    if extractor.contains("python") {
        return first_python_docstring_line(text);
    }
    None
}

fn first_rust_doc_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("///"))
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(trim_to_header)
}

/// First content line of a leading JSDoc block (`/** ... */`), with the comment
/// framing stripped. Only a block that opens the chunk contributes a synopsis.
fn first_jsdoc_line(text: &str) -> Option<String> {
    let mut lines = text.lines().map(str::trim).peekable();
    if !lines.peek()?.starts_with("/**") {
        return None;
    }
    for line in lines {
        if line == "*/" {
            break;
        }
        let content = line
            .trim_start_matches("/**")
            .trim_start_matches("/*")
            .trim_start_matches('*')
            .trim_end_matches("*/")
            .trim();
        if !content.is_empty() {
            return Some(trim_to_header(content));
        }
    }
    None
}

/// First content line of a Python docstring (`"""..."""` or `'''...'''`),
/// whether inline with the opening fence or on a following line.
fn first_python_docstring_line(text: &str) -> Option<String> {
    const FENCES: [&str; 2] = ["\"\"\"", "'''"];
    let mut lines = text.lines().map(str::trim);
    let (rest, fence) = lines.by_ref().find_map(|line| {
        FENCES
            .iter()
            .find_map(|fence| line.strip_prefix(fence).map(|rest| (rest, *fence)))
    })?;

    let inline = rest.trim_end_matches(fence).trim();
    if !inline.is_empty() {
        return Some(trim_to_header(inline));
    }
    lines
        .map(|line| line.trim_end_matches(fence).trim())
        .find(|line| !line.is_empty())
        .map(trim_to_header)
}

fn trim_to_header(text: &str) -> String {
    const MAX_HEADER_CHARS: usize = 240;
    let mut output = String::new();
    for character in text.chars().take(MAX_HEADER_CHARS) {
        output.push(character);
    }
    if text.chars().count() > MAX_HEADER_CHARS {
        output.push_str("...");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(text: &str, extractor: &str) -> SearchHit {
        SearchHit {
            chunk: ChunkRecord {
                id: "chunk-1".to_owned(),
                source_path: "src/lib.rs".to_owned(),
                source_hash: "hash".to_owned(),
                span: SourceSpan {
                    byte_start: 0,
                    byte_end: text.len(),
                    line_start: 1,
                    line_end: 3,
                },
                text: text.to_owned(),
                extractor: extractor.to_owned(),
                symbol: None,
            },
            score: 1.5,
        }
    }

    #[test]
    fn code_header_uses_signature_and_doc_synopsis() {
        let header = SearchHeader::from_hit(&hit(
            "/// Adds values.\nfn add(left: i32, right: i32) -> i32 {\n    left + right\n}",
            "tree-sitter-rust-v1",
        ));

        assert_eq!(header.chunk_id, "chunk-1");
        assert_eq!(
            header.signature.as_deref(),
            Some("fn add(left: i32, right: i32) -> i32")
        );
        assert_eq!(header.synopsis, "Adds values.");
    }

    #[test]
    fn typescript_header_skips_jsdoc_in_signature_and_synopsis() {
        let header = SearchHeader::from_hit(&hit(
            "/**\n * Adds two values.\n * @param left first\n */\nexport function add(left: number, right: number): number {\n  return left + right;\n}",
            "tree-sitter-typescript-v1",
        ));

        assert_eq!(
            header.signature.as_deref(),
            Some("export function add(left: number, right: number): number")
        );
        assert_eq!(header.synopsis, "Adds two values.");
    }

    #[test]
    fn python_header_uses_signature_and_docstring_synopsis() {
        let header = SearchHeader::from_hit(&hit(
            "def add(left, right):\n    \"\"\"Adds two values.\"\"\"\n    return left + right",
            "tree-sitter-python-v1",
        ));

        assert_eq!(header.signature.as_deref(), Some("def add(left, right):"));
        assert_eq!(header.synopsis, "Adds two values.");
    }

    #[test]
    fn prose_header_uses_first_non_empty_line() {
        let header = SearchHeader::from_hit(&hit(
            "\n\nThe first useful sentence.\nMore body text.",
            "plain-text-v1",
        ));

        assert_eq!(header.signature, None);
        assert_eq!(header.synopsis, "The first useful sentence.");
    }
}
