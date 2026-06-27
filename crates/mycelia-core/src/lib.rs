mod discovery;
mod embedding;
mod error;
mod evaluation;
mod extract;
mod model;
mod store;

pub use embedding::EmbeddingProvider;
pub use error::{Error, Result};
pub use model::{
    Chunk, ChunkRecord, CorpusStatusReport, EmbeddingReport, EvaluationCase, EvaluationCaseResult,
    EvaluationReport, ExpectedMatch, IndexReport, RetrievalStrategy, Retrieved, SearchHeader,
    SearchHit, SourceRefresh, SourceSpan, TokenUsageReport,
};

use std::path::Path;

const FIND_HEADER_LIMIT_MAX: usize = 50;
const FIND_HEADER_BYTE_BUDGET: usize = 32 * 1024;

/// True when a token looks like a code identifier rather than an English word.
/// Used to classify queries and to gate the reranker's signature-coverage signal
/// to genuine symbol tokens. Operates on the raw, case-preserving token.
pub(crate) fn is_identifier_token(token: &str) -> bool {
    let mut characters = token.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if token.contains('_') || token.contains("::") {
        return true;
    }
    let rest = &token[first.len_utf8()..];
    // camelCase / PascalCase: an interior uppercase next to lowercase.
    if rest.chars().any(|character| character.is_uppercase())
        && token.chars().any(|character| character.is_lowercase())
    {
        return true;
    }
    // ALLCAPS acronyms longer than one character (FTS5, BM25).
    token.len() > 1
        && token
            .chars()
            .all(|character| character.is_uppercase() || character.is_ascii_digit())
}

/// Indexes one local corpus into a persistent database.
pub fn index_corpus(root: &Path, database: &Path) -> Result<IndexReport> {
    store::index_corpus(root, database)
}

/// Finds chunks with the default reranked FTS5 retrieval strategy.
pub fn find(database: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    find_with_strategy(database, query, limit, RetrievalStrategy::Fts5Reranked)
}

/// Finds chunks and projects them to bounded headers without chunk bodies.
pub fn find_headers(database: &Path, query: &str, limit: usize) -> Result<Vec<SearchHeader>> {
    find_headers_with_strategy(database, query, limit, RetrievalStrategy::Fts5Reranked)
}

/// Finds chunks using the selected retrieval strategy.
pub fn find_with_strategy(
    database: &Path,
    query: &str,
    limit: usize,
    strategy: RetrievalStrategy,
) -> Result<Vec<SearchHit>> {
    store::find(database, query, limit, strategy)
}

/// Finds chunks with a selected strategy and projects them to bounded headers.
pub fn find_headers_with_strategy(
    database: &Path,
    query: &str,
    limit: usize,
    strategy: RetrievalStrategy,
) -> Result<Vec<SearchHeader>> {
    let hits = find_with_strategy(database, query, bounded_header_limit(limit)?, strategy)?;
    Ok(headers_with_budget(hits.as_slice()))
}

/// Refreshes cached embeddings for all chunks using one model.
pub fn refresh_embeddings(
    database: &Path,
    provider: &mut dyn EmbeddingProvider,
) -> Result<EmbeddingReport> {
    embedding::refresh(database, provider)
}

/// Reports whether the corpus already holds embeddings for the given model.
/// Read-only and cheap, so callers can skip loading an embedding model (and the
/// network fetch its first initialization may trigger) when it cannot help.
pub fn has_embeddings(database: &Path, model_id: &str) -> Result<bool> {
    Ok(store::embedding_count(database, model_id)? > 0)
}

/// Finds chunks using a vector or hybrid retrieval strategy.
pub fn find_with_embeddings(
    database: &Path,
    query: &str,
    limit: usize,
    strategy: RetrievalStrategy,
    provider: &mut dyn EmbeddingProvider,
) -> Result<Vec<SearchHit>> {
    embedding::find(database, query, limit, strategy, provider)
}

/// Finds chunks through embeddings and projects them to bounded headers.
pub fn find_headers_with_embeddings(
    database: &Path,
    query: &str,
    limit: usize,
    strategy: RetrievalStrategy,
    provider: &mut dyn EmbeddingProvider,
) -> Result<Vec<SearchHeader>> {
    let hits = find_with_embeddings(
        database,
        query,
        bounded_header_limit(limit)?,
        strategy,
        provider,
    )?;
    Ok(headers_with_budget(hits.as_slice()))
}

/// Retrieves one chunk by its deterministic identifier, re-validating it against
/// disk so a changed or removed source yields a structured stale signal rather
/// than outdated content. `None` means the identifier is absent from the index.
pub fn retrieve(database: &Path, chunk_id: &str) -> Result<Option<Retrieved>> {
    store::retrieve(database, chunk_id)
}

/// Reports which of the given source paths have drifted from the index on disk
/// (changed, removed, unreadable, or unbacked). Pure read: a caller validates the
/// sources behind a result set, then self-heals the drifted ones before trusting
/// the headers. Deduplicated, input order preserved.
pub fn drifted_sources(database: &Path, paths: &[String]) -> Result<Vec<String>> {
    store::drifted_sources(database, paths)
}

/// Self-heals one source file in the bound index after the query path detected
/// drift: re-chunks a changed file in place, prunes a removed or non-text file,
/// or leaves an unchanged file alone. Writes to the given database, so callers
/// reserve it for maintaining their own launch-bound corpus, never an arbitrary
/// path. New chunks carry no embedding until the next embed pass; routed
/// retrieval already falls back to reranked FTS5 for them, so lexical and exact
/// correctness are immediate.
pub fn refresh_source(database: &Path, source_path: &str) -> Result<SourceRefresh> {
    store::refresh_source(database, source_path)
}

/// Evaluates the default reranked FTS5 strategy against sourced expectations.
pub fn evaluate(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
) -> Result<EvaluationReport> {
    evaluate_with_strategy(database, cases, limit, RetrievalStrategy::Fts5Reranked)
}

/// Evaluates a selected retrieval strategy against sourced expectations.
pub fn evaluate_with_strategy(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: RetrievalStrategy,
) -> Result<EvaluationReport> {
    evaluation::evaluate(database, cases, limit, strategy)
}

/// Evaluates a vector or hybrid strategy using one initialized provider.
pub fn evaluate_with_embeddings(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: RetrievalStrategy,
    provider: &mut dyn EmbeddingProvider,
) -> Result<EvaluationReport> {
    evaluation::evaluate_with_embeddings(database, cases, limit, strategy, provider)
}

/// Returns aggregate statistics about the corpus database for the `status` command.
pub fn corpus_status(database: &Path) -> Result<CorpusStatusReport> {
    store::corpus_db_stats(database)
}

fn bounded_header_limit(limit: usize) -> Result<usize> {
    if limit == 0 {
        return Err(Error::InvalidLimit);
    }
    Ok(limit.min(FIND_HEADER_LIMIT_MAX))
}

fn headers_with_budget(hits: &[SearchHit]) -> Vec<SearchHeader> {
    let mut headers = Vec::new();
    let mut used_bytes = 0usize;

    for hit in hits {
        let header = SearchHeader::from_hit(hit);
        let header_bytes = header.approximate_bytes();
        if !headers.is_empty() && used_bytes + header_bytes > FIND_HEADER_BYTE_BUDGET {
            break;
        }
        used_bytes += header_bytes;
        headers.push(header);
    }

    headers
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    #[test]
    fn find_headers_omit_text_and_cap_requested_limit() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        for index in 0..60 {
            fs::write(
                root.join(format!("note-{index}.txt")),
                format!("shared needle {index}"),
            )
            .expect("write corpus file");
        }
        crate::index_corpus(root.as_path(), database.as_path()).expect("index corpus");

        let headers = crate::find_headers(database.as_path(), "needle", 100).expect("find");

        assert_eq!(headers.len(), 50);
        assert_eq!(headers[0].source_path, "note-0.txt");
        assert_eq!(headers[0].signature, None);
        assert_eq!(headers[0].synopsis, "shared needle 0");
    }
}
