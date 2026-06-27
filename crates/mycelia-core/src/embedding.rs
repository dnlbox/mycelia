use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::Path;
use std::time::Instant;

use crate::{ChunkRecord, EmbeddingReport, Error, Result, RetrievalStrategy, SearchHit, store};

const EMBEDDING_BATCH_SIZE: usize = 256;

/// Tunable weights for one hybrid blend. Query-class routing selects a profile
/// per query so symbol lookups stay lexical-first while prose questions lean on
/// the semantic vector signal. See [`classify_query`].
#[derive(Clone, Copy, Debug)]
struct HybridProfile {
    /// Top lexical hits force-ranked above the blend, untouched by vector noise.
    protected_lexical: usize,
    /// Multiplier on the normalized vector score when blending candidates.
    vector_weight: f64,
    /// Reranked-lexical score at or above which the query returns lexical-only,
    /// skipping the vector pass entirely. `None` disables the shortcut.
    strong_lexical_shortcut: Option<f64>,
}

impl HybridProfile {
    /// The original precision-first blend used by `RetrievalStrategy::Hybrid`.
    const fn balanced() -> Self {
        Self {
            protected_lexical: 3,
            vector_weight: 2.0,
            strong_lexical_shortcut: Some(2.0),
        }
    }

    /// Symbol-shaped queries: trust lexical, short-circuit on partial coverage,
    /// and only blend in vectors when lexical evidence is thin.
    const fn lexical() -> Self {
        Self {
            protected_lexical: 3,
            vector_weight: 1.5,
            strong_lexical_shortcut: Some(1.0),
        }
    }

    /// Prose questions: never let a spurious lexical match suppress the semantic
    /// signal, and weight the vector ranking well above reciprocal lexical rank.
    const fn semantic() -> Self {
        Self {
            protected_lexical: 1,
            vector_weight: 4.0,
            strong_lexical_shortcut: None,
        }
    }
}

/// Produces local embeddings for one identified model.
pub trait EmbeddingProvider {
    fn model_id(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn embed_documents(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn embed_query(&mut self, query: &str) -> Result<Vec<f32>>;
}

pub(crate) fn refresh(
    database: &Path,
    provider: &mut dyn EmbeddingProvider,
) -> Result<EmbeddingReport> {
    let started_at = Instant::now();
    let model_id = provider.model_id().to_owned();
    let dimensions = provider.dimensions();
    let removed_other_models = store::remove_other_model_embeddings(database, model_id.as_str())?;
    let missing = store::chunks_missing_embeddings(database, model_id.as_str())?;
    let unchanged = store::embedding_count(database, model_id.as_str())?;
    let total = missing.len();
    let mut embedded = 0usize;

    if total > 0 {
        eprint!("Embedding chunks: 0/{total}...");
        std::io::stderr().flush().ok();
    }

    for batch in missing.chunks(EMBEDDING_BATCH_SIZE) {
        let texts = batch
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let vectors = provider.embed_documents(texts.as_slice())?;
        if vectors.len() != batch.len() {
            return Err(Error::EmbeddingProvider(format!(
                "provider returned {} vectors for {} documents",
                vectors.len(),
                batch.len()
            )));
        }
        let pairs = batch
            .iter()
            .zip(vectors)
            .map(|(chunk, vector)| {
                validate_vector(vector.as_slice(), dimensions)?;
                Ok((chunk.id.clone(), vector))
            })
            .collect::<Result<Vec<_>>>()?;
        store::upsert_embedding_batch(database, model_id.as_str(), dimensions, pairs.as_slice())?;
        embedded += batch.len();
        eprint!("\rEmbedding chunks: {embedded}/{total}...");
        std::io::stderr().flush().ok();
    }

    if total > 0 {
        eprintln!();
    }

    let storage_bytes = store::embedding_storage_bytes(database, model_id.as_str())?;

    Ok(EmbeddingReport {
        model_id,
        dimensions,
        embedded,
        unchanged,
        removed_other_models,
        storage_bytes,
        elapsed_ms: started_at.elapsed().as_millis(),
    })
}

pub(crate) fn find(
    database: &Path,
    query: &str,
    limit: usize,
    strategy: RetrievalStrategy,
    provider: &mut dyn EmbeddingProvider,
) -> Result<Vec<SearchHit>> {
    let normalized_query = query.trim();
    if normalized_query.is_empty() {
        return Err(Error::EmptyQuery);
    }
    if limit == 0 {
        return Err(Error::InvalidLimit);
    }

    match strategy {
        RetrievalStrategy::Vector => find_vector(database, normalized_query, limit, provider),
        RetrievalStrategy::Hybrid => find_hybrid(
            database,
            normalized_query,
            limit,
            provider,
            HybridProfile::balanced(),
        ),
        RetrievalStrategy::Routed => find_routed(database, normalized_query, limit, provider),
        _ => store::find(database, normalized_query, limit, strategy),
    }
}

/// The shape of a query, used to pick a [`HybridProfile`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueryClass {
    /// A bag of identifiers (`chunk_record_from_row`, `RunStore`) with no prose.
    SymbolLike,
    /// A natural-language question or paraphrase ("function that reads a row").
    NaturalLanguage,
}

/// Function words that signal prose rather than a symbol lookup.
const QUERY_STOP_WORDS: &[&str] = &[
    "the",
    "a",
    "an",
    "that",
    "for",
    "of",
    "to",
    "in",
    "on",
    "by",
    "with",
    "based",
    "its",
    "and",
    "or",
    "when",
    "how",
    "what",
    "which",
    "using",
    "alongside",
    "across",
    "into",
    "at",
    "is",
    "are",
    "be",
    "from",
    "as",
    "this",
    "it",
    "instead",
    "rather",
    "than",
    "before",
    "after",
    "without",
    "not",
    "should",
    "must",
    "only",
    "your",
];

/// Classifies a query as a symbol lookup or a natural-language question.
///
/// Prose carries function words; symbol queries are identifier tokens
/// (`snake_case`, `camelCase`, `PascalCase`, `Foo::bar`, `ALLCAPS`). Two or more
/// stop words, or a longer phrase with any stop word, reads as natural language.
fn classify_query(query: &str) -> QueryClass {
    let tokens = query
        .split_whitespace()
        .map(|token| token.trim_matches(|character: char| !character.is_alphanumeric()))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let stop_words = tokens
        .iter()
        .filter(|token| QUERY_STOP_WORDS.contains(&token.to_lowercase().as_str()))
        .count();
    let identifier_tokens = tokens
        .iter()
        .filter(|token| crate::is_identifier_token(token))
        .count();

    if stop_words >= 2 {
        return QueryClass::NaturalLanguage;
    }
    if identifier_tokens >= 1 && stop_words == 0 {
        return QueryClass::SymbolLike;
    }
    if tokens.len() >= 6 && stop_words >= 1 {
        return QueryClass::NaturalLanguage;
    }
    QueryClass::SymbolLike
}

/// Routes a query to a class-specific [`HybridProfile`], falling back to lexical
/// reranking when no embeddings are available.
fn find_routed(
    database: &Path,
    query: &str,
    limit: usize,
    provider: &mut dyn EmbeddingProvider,
) -> Result<Vec<SearchHit>> {
    let profile = match classify_query(query) {
        QueryClass::SymbolLike => HybridProfile::lexical(),
        QueryClass::NaturalLanguage => HybridProfile::semantic(),
    };
    match find_hybrid(database, query, limit, provider, profile) {
        Err(Error::MissingEmbeddings(_)) => {
            store::find(database, query, limit, RetrievalStrategy::Fts5Reranked)
        }
        other => other,
    }
}

fn find_vector(
    database: &Path,
    query: &str,
    limit: usize,
    provider: &mut dyn EmbeddingProvider,
) -> Result<Vec<SearchHit>> {
    let dimensions = provider.dimensions();
    let query_vector = provider.embed_query(query)?;
    validate_vector(query_vector.as_slice(), dimensions)?;
    let stored = store::load_embedding_vectors(database, provider.model_id(), dimensions)?;
    if stored.is_empty() {
        return Err(Error::MissingEmbeddings(provider.model_id().to_owned()));
    }

    // Score every stored vector, then keep only the top `limit` before loading
    // chunk bodies, so a query clones at most `limit` records rather than the
    // whole corpus on every call.
    let mut scored = Vec::with_capacity(stored.len());
    for embedding in stored {
        validate_vector(embedding.vector.as_slice(), dimensions)?;
        let score = cosine_similarity(query_vector.as_slice(), embedding.vector.as_slice());
        scored.push((embedding, score));
    }
    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left.source_path.cmp(&right.source_path))
            .then_with(|| left.byte_start.cmp(&right.byte_start))
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    scored.truncate(limit);

    let ids = scored
        .iter()
        .map(|(embedding, _)| embedding.chunk_id.clone())
        .collect::<Vec<_>>();
    let mut records = store::chunks_by_ids(database, ids.as_slice())?;
    let hits = scored
        .into_iter()
        .filter_map(|(embedding, score)| {
            records
                .remove(&embedding.chunk_id)
                .map(|chunk| SearchHit { chunk, score })
        })
        .collect();
    Ok(hits)
}

fn find_hybrid(
    database: &Path,
    query: &str,
    limit: usize,
    provider: &mut dyn EmbeddingProvider,
    profile: HybridProfile,
) -> Result<Vec<SearchHit>> {
    let candidate_limit = limit.saturating_mul(20).clamp(100, 1_000);
    let lexical = match store::find(
        database,
        query,
        candidate_limit,
        RetrievalStrategy::Fts5Reranked,
    ) {
        Ok(hits) => hits,
        Err(Error::NoSearchTerms) => Vec::new(),
        Err(error) => return Err(error),
    };
    if let Some(threshold) = profile.strong_lexical_shortcut
        && lexical.len() >= limit
        && lexical.first().is_some_and(|hit| hit.score >= threshold)
    {
        let mut hits = lexical;
        hits.truncate(limit);
        return Ok(hits);
    }

    let vector = find_vector(database, query, candidate_limit, provider)?;
    let protected_count = lexical.len().min(limit).min(profile.protected_lexical);
    let protected_ids = lexical
        .iter()
        .take(protected_count)
        .map(|hit| hit.chunk.id.clone())
        .collect::<BTreeSet<_>>();
    let mut hits = lexical
        .iter()
        .take(protected_count)
        .enumerate()
        .map(|(index, hit)| SearchHit {
            chunk: hit.chunk.clone(),
            score: protected_lexical_score(index),
        })
        .collect::<Vec<_>>();
    let mut candidates = BTreeMap::<String, HybridCandidate>::new();

    for (index, hit) in lexical.into_iter().enumerate().skip(protected_count) {
        add_lexical_candidate(&mut candidates, hit, index + 1);
    }
    for (index, hit) in vector.into_iter().enumerate() {
        if !protected_ids.contains(hit.chunk.id.as_str()) {
            add_vector_candidate(&mut candidates, hit, index + 1);
        }
    }

    let mut ranked = candidates.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| compare_hybrid_candidates(left, right, profile.vector_weight));
    hits.extend(
        ranked
            .into_iter()
            .map(|candidate| candidate.into_hit(profile.vector_weight)),
    );
    hits.truncate(limit);
    Ok(hits)
}

#[derive(Debug)]
struct HybridCandidate {
    chunk: ChunkRecord,
    lexical_rank: Option<usize>,
    vector_rank: Option<usize>,
    vector_score: Option<f64>,
}

impl HybridCandidate {
    fn score(&self, vector_weight: f64) -> f64 {
        let lexical = self
            .lexical_rank
            .map(|rank| 1.0 / rank as f64)
            .unwrap_or(0.0);
        let vector = self
            .vector_score
            .map(|score| normalized_vector_score(score) * vector_weight)
            .unwrap_or(0.0);
        lexical + vector
    }

    fn into_hit(self, vector_weight: f64) -> SearchHit {
        SearchHit {
            score: self.score(vector_weight),
            chunk: self.chunk,
        }
    }
}

fn add_lexical_candidate(
    candidates: &mut BTreeMap<String, HybridCandidate>,
    hit: SearchHit,
    rank: usize,
) {
    candidates
        .entry(hit.chunk.id.clone())
        .and_modify(|candidate| {
            candidate.lexical_rank = Some(
                candidate
                    .lexical_rank
                    .map_or(rank, |current| current.min(rank)),
            );
        })
        .or_insert(HybridCandidate {
            chunk: hit.chunk,
            lexical_rank: Some(rank),
            vector_rank: None,
            vector_score: None,
        });
}

fn add_vector_candidate(
    candidates: &mut BTreeMap<String, HybridCandidate>,
    hit: SearchHit,
    rank: usize,
) {
    candidates
        .entry(hit.chunk.id.clone())
        .and_modify(|candidate| {
            candidate.vector_rank = Some(
                candidate
                    .vector_rank
                    .map_or(rank, |current| current.min(rank)),
            );
            candidate.vector_score = Some(
                candidate
                    .vector_score
                    .map_or(hit.score, |current| current.max(hit.score)),
            );
        })
        .or_insert(HybridCandidate {
            chunk: hit.chunk,
            lexical_rank: None,
            vector_rank: Some(rank),
            vector_score: Some(hit.score),
        });
}

fn compare_hybrid_candidates(
    left: &HybridCandidate,
    right: &HybridCandidate,
    vector_weight: f64,
) -> std::cmp::Ordering {
    right
        .score(vector_weight)
        .total_cmp(&left.score(vector_weight))
        .then_with(|| match (left.lexical_rank, right.lexical_rank) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left
                .vector_rank
                .unwrap_or(usize::MAX)
                .cmp(&right.vector_rank.unwrap_or(usize::MAX)),
        })
        .then_with(|| left.chunk.source_path.cmp(&right.chunk.source_path))
        .then_with(|| left.chunk.span.byte_start.cmp(&right.chunk.span.byte_start))
        .then_with(|| left.chunk.id.cmp(&right.chunk.id))
}

fn protected_lexical_score(index: usize) -> f64 {
    10.0 + 1.0 / (index + 1) as f64
}

fn normalized_vector_score(score: f64) -> f64 {
    ((score + 1.0) / 2.0).clamp(0.0, 1.0)
}

fn validate_vector(vector: &[f32], dimensions: usize) -> Result<()> {
    if vector.len() != dimensions {
        return Err(Error::EmbeddingDimensions {
            expected: dimensions,
            found: vector.len(),
        });
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(Error::NonFiniteEmbedding);
    }
    Ok(())
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f64 {
    let (dot, left_norm, right_norm) = left.iter().zip(right).fold(
        (0.0_f64, 0.0_f64, 0.0_f64),
        |(dot, left_norm, right_norm), (left, right)| {
            let left = f64::from(*left);
            let right = f64::from(*right);
            (
                dot + left * right,
                left_norm + left * left,
                right_norm + right * right,
            )
        },
    );
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm.sqrt() * right_norm.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    struct KeywordProvider;

    impl EmbeddingProvider for KeywordProvider {
        fn model_id(&self) -> &str {
            "keyword-test-v1"
        }

        fn dimensions(&self) -> usize {
            2
        }

        fn embed_documents(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|text| embed_keywords(text)).collect())
        }

        fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
            Ok(embed_keywords(query))
        }
    }

    struct ReplacementProvider;

    impl EmbeddingProvider for ReplacementProvider {
        fn model_id(&self) -> &str {
            "keyword-test-v2"
        }

        fn dimensions(&self) -> usize {
            2
        }

        fn embed_documents(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|text| embed_keywords(text)).collect())
        }

        fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
            Ok(embed_keywords(query))
        }
    }

    /// Embeds the first batch, then fails — used to prove that batches already
    /// upserted survive a later failure so `refresh` can resume.
    struct FailAfterFirstBatch {
        calls: usize,
    }

    impl EmbeddingProvider for FailAfterFirstBatch {
        fn model_id(&self) -> &str {
            "keyword-test-v1"
        }

        fn dimensions(&self) -> usize {
            2
        }

        fn embed_documents(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            self.calls += 1;
            if self.calls > 1 {
                return Err(Error::EmbeddingProvider(
                    "injected batch failure".to_owned(),
                ));
            }
            Ok(texts.iter().map(|text| embed_keywords(text)).collect())
        }

        fn embed_query(&mut self, _query: &str) -> Result<Vec<f32>> {
            Err(Error::EmbeddingProvider("query not supported".to_owned()))
        }
    }

    fn embed_keywords(text: &str) -> Vec<f32> {
        vec![
            if text.contains("alpha") || text.contains("first") {
                1.0
            } else {
                0.0
            },
            if text.contains("beta") || text.contains("second") {
                1.0
            } else {
                0.0
            },
        ]
    }

    #[test]
    fn refresh_persists_completed_batches_when_a_later_batch_fails() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");

        // More than one embedding batch, so the injected failure lands after
        // the first batch has already been committed.
        let paragraphs = EMBEDDING_BATCH_SIZE + 40;
        let mut body = String::new();
        for index in 0..paragraphs {
            body.push_str(&format!("alpha paragraph {index}\n\n"));
        }
        fs::write(root.join("a.txt"), body).expect("write corpus");
        crate::index_corpus(&root, &database).expect("index");

        let model_id = KeywordProvider.model_id().to_owned();
        let total = store::chunks_missing_embeddings(&database, &model_id)
            .expect("missing")
            .len();
        assert!(
            total > EMBEDDING_BATCH_SIZE,
            "need more than one batch, got {total}"
        );

        let mut faulty = FailAfterFirstBatch { calls: 0 };
        let error = refresh(&database, &mut faulty).expect_err("second batch must fail");
        assert!(matches!(error, Error::EmbeddingProvider(_)));

        // The first batch was upserted before the failure, so it survives.
        assert_eq!(
            store::embedding_count(&database, &model_id).expect("count"),
            EMBEDDING_BATCH_SIZE
        );

        // A clean re-run resumes and embeds only the remainder.
        let mut provider = KeywordProvider;
        let report = refresh(&database, &mut provider).expect("resume refresh");
        assert_eq!(report.embedded, total - EMBEDDING_BATCH_SIZE);
        assert_eq!(report.unchanged, EMBEDDING_BATCH_SIZE);
        assert_eq!(
            store::embedding_count(&database, &model_id).expect("count"),
            total
        );
    }

    #[test]
    fn refresh_reuses_unchanged_embeddings_and_vector_search_preserves_provenance() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "alpha content").expect("write alpha");
        fs::write(root.join("b.txt"), "beta content").expect("write beta");
        crate::index_corpus(&root, &database).expect("index");
        let mut provider = KeywordProvider;

        let first = refresh(&database, &mut provider).expect("first refresh");
        let second = refresh(&database, &mut provider).expect("second refresh");
        let hits = find(
            &database,
            "first",
            2,
            RetrievalStrategy::Vector,
            &mut provider,
        )
        .expect("vector find");
        let ties = find(
            &database,
            "unknown",
            2,
            RetrievalStrategy::Vector,
            &mut provider,
        )
        .expect("tied vector find");

        assert_eq!(first.embedded, 2);
        assert_eq!(first.unchanged, 0);
        assert_eq!(second.embedded, 0);
        assert_eq!(second.unchanged, 2);
        assert_eq!(hits[0].chunk.source_path, "a.txt");
        assert_eq!(hits[0].chunk.span.line_start, 1);
        assert_eq!(ties[0].chunk.source_path, "a.txt");
        assert_eq!(ties[1].chunk.source_path, "b.txt");
    }

    #[test]
    fn changing_models_replaces_the_previous_embedding_cache() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "alpha content").expect("write alpha");
        crate::index_corpus(&root, &database).expect("index");
        let mut first_provider = KeywordProvider;
        let mut replacement_provider = ReplacementProvider;

        refresh(&database, &mut first_provider).expect("first model");
        let replacement = refresh(&database, &mut replacement_provider).expect("replacement model");

        assert_eq!(replacement.removed_other_models, 1);
        assert_eq!(
            store::embedding_count(&database, first_provider.model_id()).expect("old count"),
            0
        );
        assert_eq!(
            store::embedding_count(&database, replacement_provider.model_id())
                .expect("replacement count"),
            1
        );
    }

    #[test]
    fn vector_search_requires_an_explicit_embedding_refresh() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "alpha content").expect("write alpha");
        crate::index_corpus(&root, &database).expect("index");
        let mut provider = KeywordProvider;

        assert!(matches!(
            find(
                &database,
                "alpha",
                1,
                RetrievalStrategy::Vector,
                &mut provider
            ),
            Err(Error::MissingEmbeddings(model_id)) if model_id == "keyword-test-v1"
        ));
    }

    #[test]
    fn hybrid_recovers_semantic_match_without_losing_lexical_match() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "alpha exact").expect("write alpha");
        fs::write(root.join("b.txt"), "beta exact").expect("write beta");
        crate::index_corpus(&root, &database).expect("index");
        let mut provider = KeywordProvider;
        refresh(&database, &mut provider).expect("refresh");

        let semantic = find(
            &database,
            "first",
            1,
            RetrievalStrategy::Hybrid,
            &mut provider,
        )
        .expect("semantic hybrid");
        let lexical = find(
            &database,
            "beta",
            1,
            RetrievalStrategy::Hybrid,
            &mut provider,
        )
        .expect("lexical hybrid");

        assert_eq!(semantic[0].chunk.source_path, "a.txt");
        assert_eq!(lexical[0].chunk.source_path, "b.txt");
    }

    #[test]
    fn hybrid_appends_semantic_expansion_below_protected_lexical_hits() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "alpha semantic expansion").expect("write alpha");
        fs::write(root.join("b.txt"), "beta exact primary").expect("write beta");
        fs::write(root.join("c.txt"), "beta exact secondary").expect("write beta");
        crate::index_corpus(&root, &database).expect("index");
        let mut provider = KeywordProvider;
        refresh(&database, &mut provider).expect("refresh");

        let protected = find(
            &database,
            "beta first",
            2,
            RetrievalStrategy::Hybrid,
            &mut provider,
        )
        .expect("protected hybrid");
        let expanded = find(
            &database,
            "beta first",
            3,
            RetrievalStrategy::Hybrid,
            &mut provider,
        )
        .expect("expanded hybrid");

        assert_eq!(
            protected
                .iter()
                .map(|hit| hit.chunk.source_path.as_str())
                .collect::<Vec<_>>(),
            vec!["b.txt", "c.txt"]
        );
        assert_eq!(
            expanded
                .iter()
                .map(|hit| hit.chunk.source_path.as_str())
                .collect::<Vec<_>>(),
            vec!["b.txt", "c.txt", "a.txt"]
        );
    }

    #[test]
    fn classify_query_separates_symbol_lookups_from_prose() {
        // Bags of identifiers read as symbol lookups.
        assert_eq!(
            classify_query("chunk_record_from_row ChunkRecord rusqlite Row"),
            QueryClass::SymbolLike
        );
        assert_eq!(
            classify_query("RunStore persist FTS5"),
            QueryClass::SymbolLike
        );
        // Bare keyword phrases with no function words stay lexical.
        assert_eq!(
            classify_query("self-organizing agentic workspace blueprint"),
            QueryClass::SymbolLike
        );
        // Prose with function words reads as natural language.
        assert_eq!(
            classify_query("return the extractor identifier for a source path"),
            QueryClass::NaturalLanguage
        );
        assert_eq!(
            classify_query("function that reads a row and returns a record"),
            QueryClass::NaturalLanguage
        );
    }

    #[test]
    fn routed_uses_lexical_for_symbols_and_semantic_for_prose() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "alpha exact").expect("write alpha");
        fs::write(root.join("b.txt"), "beta exact").expect("write beta");
        crate::index_corpus(&root, &database).expect("index");
        let mut provider = KeywordProvider;
        refresh(&database, &mut provider).expect("refresh");

        // Symbol-shaped query routes through the lexical profile.
        let symbol = find(
            &database,
            "beta",
            1,
            RetrievalStrategy::Routed,
            &mut provider,
        )
        .expect("routed symbol");
        // Prose query (two stop words) routes through the semantic profile and
        // recovers the vector match for "second" -> beta.
        let prose = find(
            &database,
            "find the item that is second",
            1,
            RetrievalStrategy::Routed,
            &mut provider,
        )
        .expect("routed prose");

        assert_eq!(symbol[0].chunk.source_path, "b.txt");
        assert_eq!(prose[0].chunk.source_path, "b.txt");
    }

    #[test]
    fn routed_falls_back_to_lexical_when_embeddings_are_missing() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "alpha exact").expect("write alpha");
        crate::index_corpus(&root, &database).expect("index");
        let mut provider = KeywordProvider;

        // Prose query needs the vector pass, but no embeddings exist yet: routing
        // must degrade to lexical reranking instead of erroring.
        let hits = find(
            &database,
            "find the row that is alpha",
            1,
            RetrievalStrategy::Routed,
            &mut provider,
        )
        .expect("routed without embeddings");

        assert_eq!(hits[0].chunk.source_path, "a.txt");
    }
}
