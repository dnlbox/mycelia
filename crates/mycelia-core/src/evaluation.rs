use std::cmp::Ordering;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use crate::{
    BaselineEvaluationReport, EmbeddingProvider, Error, EvaluationCase, EvaluationCaseResult,
    EvaluationComparison, EvaluationReport, ExpectedMatch, PairedEvaluationReport, Result,
    RetrievalStrategy, SearchHeader, SearchHit, TokenUsageReport, discovery, store,
};

pub(crate) fn evaluate(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: RetrievalStrategy,
) -> Result<EvaluationReport> {
    let corpus_root = store::corpus_root(database)?;
    evaluate_cases(
        database,
        cases,
        limit,
        strategy,
        corpus_root.as_ref(),
        |query| crate::find_with_strategy(database, query, limit, strategy),
    )
}

pub(crate) fn evaluate_with_embeddings(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: RetrievalStrategy,
    provider: &mut dyn EmbeddingProvider,
) -> Result<EvaluationReport> {
    let corpus_root = store::corpus_root(database)?;
    evaluate_cases(
        database,
        cases,
        limit,
        strategy,
        corpus_root.as_ref(),
        |query| crate::find_with_embeddings(database, query, limit, strategy, provider),
    )
}

pub(crate) fn evaluate_baseline(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
) -> Result<BaselineEvaluationReport> {
    let corpus_root = store::corpus_root(database)?.ok_or(Error::MissingCorpusRoot)?;
    evaluate_baseline_cases(cases, limit, corpus_root.as_path())
}

pub(crate) fn pair_reports(
    mycelia: EvaluationReport,
    baseline: BaselineEvaluationReport,
) -> PairedEvaluationReport {
    let tokens_per_answer_delta =
        mycelia.token_usage.tokens_per_answer - baseline.token_usage.tokens_per_answer;
    let token_reduction_ratio = if baseline.token_usage.tokens_per_answer > 0.0 {
        Some(1.0 - (mycelia.token_usage.tokens_per_answer / baseline.token_usage.tokens_per_answer))
    } else {
        None
    };

    PairedEvaluationReport {
        comparison: EvaluationComparison {
            hit_rate_delta: mycelia.hit_rate - baseline.hit_rate,
            mean_reciprocal_rank_delta: mycelia.mean_reciprocal_rank
                - baseline.mean_reciprocal_rank,
            tokens_per_answer_delta,
            token_reduction_ratio,
        },
        mycelia,
        baseline,
    }
}

fn evaluate_cases<F>(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: RetrievalStrategy,
    corpus_root: Option<&PathBuf>,
    mut find: F,
) -> Result<EvaluationReport>
where
    F: FnMut(&str) -> Result<Vec<SearchHit>>,
{
    let started_at = Instant::now();
    if limit == 0 {
        return Err(Error::InvalidLimit);
    }

    let mut results = Vec::with_capacity(cases.len());
    let mut reciprocal_rank_sum = 0.0;
    let mut hits = 0;
    let mut token_usage = TokenUsageAccumulator::default();

    for case in cases {
        let expected = expected_matches(case);
        if expected.is_empty() {
            return Err(Error::EvaluationCaseWithoutExpected(case.name.clone()));
        }

        let rank = if !case.changed_paths.is_empty() {
            // Change-scoped retrieval: blast_radius instead of text find.
            let headers = store::blast_radius(database, &case.changed_paths, limit)?;
            let rank = first_relevant_header_rank(headers.as_slice(), expected.as_slice());
            if let Some(rank) = rank {
                hits += 1;
                reciprocal_rank_sum += 1.0 / rank as f64;
                token_usage.add_answer_headers(headers.as_slice(), corpus_root);
            }
            rank
        } else {
            let found = find(case.query.as_str())?;
            let rank = first_relevant_rank(found.as_slice(), expected.as_slice());
            if let Some(rank) = rank {
                hits += 1;
                reciprocal_rank_sum += 1.0 / rank as f64;
                token_usage.add_answer(found.as_slice(), rank - 1, corpus_root);
            }
            rank
        };
        results.push(EvaluationCaseResult {
            name: case.name.clone(),
            query: case.query.clone(),
            required_files: required_file_paths(case),
            rank,
        });
    }

    let case_count = cases.len();
    let denominator = case_count as f64;
    Ok(EvaluationReport {
        strategy,
        limit,
        cases: case_count,
        hits,
        hit_rate: if case_count == 0 {
            0.0
        } else {
            hits as f64 / denominator
        },
        mean_reciprocal_rank: if case_count == 0 {
            0.0
        } else {
            reciprocal_rank_sum / denominator
        },
        elapsed_ms: started_at.elapsed().as_millis(),
        token_usage: token_usage.finish(),
        results,
    })
}

fn evaluate_baseline_cases(
    cases: &[EvaluationCase],
    limit: usize,
    corpus_root: &Path,
) -> Result<BaselineEvaluationReport> {
    let started_at = Instant::now();
    if limit == 0 {
        return Err(Error::InvalidLimit);
    }

    let mut results = Vec::with_capacity(cases.len());
    let mut reciprocal_rank_sum = 0.0;
    let mut hits = 0;
    let mut token_usage = BaselineTokenUsageAccumulator::default();

    for case in cases {
        let expected = expected_matches(case);
        if expected.is_empty() {
            return Err(Error::EvaluationCaseWithoutExpected(case.name.clone()));
        }

        let ranked = grep_read_rank(corpus_root, case.query.as_str(), limit)?;
        let rank = first_relevant_file_rank(ranked.as_slice(), expected.as_slice());
        if let Some(rank) = rank {
            hits += 1;
            reciprocal_rank_sum += 1.0 / rank as f64;
            token_usage.add_answer(ranked.as_slice(), rank - 1);
        }
        results.push(EvaluationCaseResult {
            name: case.name.clone(),
            query: case.query.clone(),
            required_files: required_file_paths(case),
            rank,
        });
    }

    let case_count = cases.len();
    let denominator = case_count as f64;
    Ok(BaselineEvaluationReport {
        name: "grep_read".to_owned(),
        limit,
        cases: case_count,
        hits,
        hit_rate: if case_count == 0 {
            0.0
        } else {
            hits as f64 / denominator
        },
        mean_reciprocal_rank: if case_count == 0 {
            0.0
        } else {
            reciprocal_rank_sum / denominator
        },
        elapsed_ms: started_at.elapsed().as_millis(),
        token_usage: token_usage.finish(),
        results,
    })
}

#[derive(Clone, Debug)]
struct BaselineHit {
    source_path: String,
    text: String,
    bytes_read: usize,
    score: f64,
}

fn grep_read_rank(root: &Path, query: &str, limit: usize) -> Result<Vec<BaselineHit>> {
    let query_tokens = unique_tokens(tokenize(query).as_slice())?;
    let query_sequence = tokenize(query);
    let lowercase_query = query.trim().to_lowercase();
    let discovery = discovery::discover(root)?;
    let canonical_root = std::fs::canonicalize(root).map_err(|source| Error::io(root, source))?;
    let mut hits = Vec::new();

    for path in discovery.files {
        let relative = path
            .strip_prefix(&canonical_root)
            .map_err(|_| Error::PathOutsideRoot(path.clone()))?;
        let source_path = relative
            .to_str()
            .ok_or_else(|| Error::NonUtf8Path(relative.to_path_buf()))?
            .to_owned();
        let Ok(text) = std::fs::read_to_string(path.as_path()) else {
            continue;
        };
        let text_tokens = tokenize(text.as_str());
        let matched_tokens = query_tokens
            .iter()
            .filter(|token| text_tokens.contains(*token))
            .count();
        if matched_tokens == 0 {
            continue;
        }

        let phrase_matches = if lowercase_query.is_empty() {
            0.0
        } else {
            text.to_lowercase()
                .matches(lowercase_query.as_str())
                .count() as f64
        };
        let sequence_match =
            contains_token_sequence(text_tokens.as_slice(), query_sequence.as_slice());
        let score = phrase_matches * 100.0
            + if sequence_match { 50.0 } else { 0.0 }
            + matched_tokens as f64 / query_tokens.len() as f64
            + text_tokens
                .iter()
                .filter(|token| query_tokens.contains(*token))
                .count() as f64
                * 0.01;

        hits.push(BaselineHit {
            source_path,
            bytes_read: text.len(),
            text,
            score,
        });
    }

    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    hits.truncate(limit);
    Ok(hits)
}

fn first_relevant_file_rank(hits: &[BaselineHit], expected: &[ExpectedMatch]) -> Option<usize> {
    hits.iter()
        .position(|hit| {
            expected.iter().any(|target| {
                hit.source_path == target.source_path
                    && target
                        .contains
                        .as_ref()
                        .is_none_or(|fragment| hit.text.contains(fragment))
            })
        })
        .map(|index| index + 1)
}

#[derive(Default)]
struct BaselineTokenUsageAccumulator {
    answered_queries: usize,
    answer_tokens: usize,
}

impl BaselineTokenUsageAccumulator {
    fn add_answer(&mut self, hits: &[BaselineHit], relevant_index: usize) {
        let read_tokens = hits
            .iter()
            .take(relevant_index + 1)
            .map(|hit| estimated_tokens(hit.bytes_read))
            .sum::<usize>();
        self.answered_queries += 1;
        self.answer_tokens += read_tokens;
    }

    fn finish(self) -> TokenUsageReport {
        let denominator = self.answered_queries as f64;
        TokenUsageReport {
            answered_queries: self.answered_queries,
            find_header_tokens: 0,
            retrieved_body_tokens: self.answer_tokens,
            answer_tokens: self.answer_tokens,
            tokens_per_answer: if self.answered_queries == 0 {
                0.0
            } else {
                self.answer_tokens as f64 / denominator
            },
            cold_source_tokens: Some(self.answer_tokens),
            cold_tokens_per_answer: Some(if self.answered_queries == 0 {
                0.0
            } else {
                self.answer_tokens as f64 / denominator
            }),
        }
    }
}

fn expected_matches(case: &EvaluationCase) -> Vec<ExpectedMatch> {
    if !case.required_files.is_empty() {
        return case
            .required_files
            .iter()
            .cloned()
            .map(|source_path| ExpectedMatch {
                source_path,
                contains: None,
            })
            .collect();
    }

    case.expected.clone()
}

fn required_file_paths(case: &EvaluationCase) -> Vec<String> {
    if !case.required_files.is_empty() {
        return case.required_files.clone();
    }

    let mut paths = Vec::new();
    for expected in &case.expected {
        if !paths.contains(&expected.source_path) {
            paths.push(expected.source_path.clone());
        }
    }
    paths
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn unique_tokens(tokens: &[String]) -> Result<Vec<String>> {
    let mut unique = Vec::new();
    for token in tokens {
        if !unique.contains(token) {
            unique.push(token.clone());
        }
    }
    if unique.is_empty() {
        return Err(Error::NoSearchTerms);
    }
    Ok(unique)
}

fn contains_token_sequence(text: &[String], query: &[String]) -> bool {
    query.len() <= text.len() && text.windows(query.len()).any(|window| window == query)
}

fn first_relevant_rank(hits: &[SearchHit], expected: &[ExpectedMatch]) -> Option<usize> {
    hits.iter()
        .position(|hit| {
            expected.iter().any(|target| {
                hit.chunk.source_path == target.source_path
                    && target
                        .contains
                        .as_ref()
                        .is_none_or(|fragment| hit.chunk.text.contains(fragment))
            })
        })
        .map(|index| index + 1)
}

/// Rank check for `blast_radius` results (`SearchHeader` instead of `SearchHit`).
/// `contains` text checks are skipped for headers (body text is not stored).
fn first_relevant_header_rank(
    headers: &[SearchHeader],
    expected: &[ExpectedMatch],
) -> Option<usize> {
    headers
        .iter()
        .position(|h| {
            expected
                .iter()
                .any(|target| h.source_path == target.source_path)
        })
        .map(|index| index + 1)
}

#[derive(Default)]
struct TokenUsageAccumulator {
    answered_queries: usize,
    find_header_tokens: usize,
    retrieved_body_tokens: usize,
    cold_source_tokens: Option<usize>,
}

impl TokenUsageAccumulator {
    fn add_answer(
        &mut self,
        hits: &[SearchHit],
        relevant_index: usize,
        corpus_root: Option<&PathBuf>,
    ) {
        let header_tokens = hits
            .iter()
            .map(SearchHeader::from_hit)
            .map(|header| estimated_tokens(header.approximate_bytes()))
            .sum::<usize>();
        let retrieved_body_tokens = hits
            .get(relevant_index)
            .map(|hit| estimated_tokens(hit.chunk.text.len()))
            .unwrap_or(0);

        self.answered_queries += 1;
        self.find_header_tokens += header_tokens;
        self.retrieved_body_tokens += retrieved_body_tokens;

        if let (Some(root), Some(hit)) = (corpus_root, hits.get(relevant_index))
            && let Some(path) = resolve_under_root(root, hit.chunk.source_path.as_str())
            && let Ok(bytes) = std::fs::read(path)
        {
            let tokens = estimated_tokens(bytes.len());
            self.cold_source_tokens = Some(self.cold_source_tokens.unwrap_or(0) + tokens);
        }
    }

    /// Accounts for a successful `blast_radius` (change-scoped) answer.
    /// All returned headers are billed as header tokens; body retrieval is 0
    /// (the agent receives the lean blast-radius headers and decides which
    /// files to retrieve next, rather than reading a full body here).
    fn add_answer_headers(&mut self, headers: &[SearchHeader], corpus_root: Option<&PathBuf>) {
        let header_tokens = headers
            .iter()
            .map(|h| estimated_tokens(h.approximate_bytes()))
            .sum::<usize>();
        self.answered_queries += 1;
        self.find_header_tokens += header_tokens;
        // Cold-source: read the first changed-path file (score 1.0 headers come
        // first in blast_radius output, ordered by source_path/line).
        if let Some(first) = headers.first()
            && let Some(root) = corpus_root
            && let Some(path) = resolve_under_root(root, first.source_path.as_str())
            && let Ok(bytes) = std::fs::read(path)
        {
            let tokens = estimated_tokens(bytes.len());
            self.cold_source_tokens = Some(self.cold_source_tokens.unwrap_or(0) + tokens);
        }
    }

    fn finish(self) -> TokenUsageReport {
        let answer_tokens = self.find_header_tokens + self.retrieved_body_tokens;
        let denominator = self.answered_queries as f64;
        TokenUsageReport {
            answered_queries: self.answered_queries,
            find_header_tokens: self.find_header_tokens,
            retrieved_body_tokens: self.retrieved_body_tokens,
            answer_tokens,
            tokens_per_answer: if self.answered_queries == 0 {
                0.0
            } else {
                answer_tokens as f64 / denominator
            },
            cold_source_tokens: self.cold_source_tokens,
            cold_tokens_per_answer: self.cold_source_tokens.map(|tokens| {
                if self.answered_queries == 0 {
                    0.0
                } else {
                    tokens as f64 / denominator
                }
            }),
        }
    }
}

fn estimated_tokens(byte_count: usize) -> usize {
    byte_count.div_ceil(4)
}

/// Resolves a database-stored relative source path under the corpus root.
/// Returns `None` for absolute paths, parent traversal, or anything that
/// escapes the root, so a tampered or corrupt index cannot drive cold-source
/// reads outside the corpus during token accounting.
fn resolve_under_root(root: &Path, source_path: &str) -> Option<PathBuf> {
    let relative = Path::new(source_path);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }

    let canonical_root = std::fs::canonicalize(root).ok()?;
    let canonical = std::fs::canonicalize(canonical_root.join(relative)).ok()?;
    canonical.starts_with(&canonical_root).then_some(canonical)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn reports_hit_rate_and_reciprocal_rank() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "needle needle").expect("write a");
        fs::write(root.join("b.txt"), "needle").expect("write b");
        crate::index_corpus(root.as_path(), database.as_path()).expect("index");

        let cases = vec![
            EvaluationCase {
                name: "second result".to_owned(),
                query: "needle".to_owned(),
                changed_paths: Vec::new(),
                required_files: Vec::new(),
                expected: vec![ExpectedMatch {
                    source_path: "b.txt".to_owned(),
                    contains: None,
                }],
            },
            EvaluationCase {
                name: "miss".to_owned(),
                query: "needle".to_owned(),
                changed_paths: Vec::new(),
                required_files: Vec::new(),
                expected: vec![ExpectedMatch {
                    source_path: "missing.txt".to_owned(),
                    contains: None,
                }],
            },
        ];

        let report = evaluate(
            database.as_path(),
            cases.as_slice(),
            10,
            RetrievalStrategy::Substring,
        )
        .expect("evaluate");

        assert_eq!(report.hits, 1);
        assert_eq!(report.results[0].rank, Some(2));
        assert_eq!(report.results[0].required_files, vec!["b.txt"]);
        assert_eq!(report.results[1].rank, None);
        assert!((report.hit_rate - 0.5).abs() < f64::EPSILON);
        assert!((report.mean_reciprocal_rank - 0.25).abs() < f64::EPSILON);

        // Token accounting: only the answered query contributes, and it bills
        // the headers scanned to the hit plus the retrieved body.
        let tokens = &report.token_usage;
        assert_eq!(tokens.answered_queries, 1);
        assert!(tokens.find_header_tokens > 0, "headers should cost tokens");
        assert!(
            tokens.retrieved_body_tokens > 0,
            "retrieved body should cost tokens"
        );
        assert_eq!(
            tokens.answer_tokens,
            tokens.find_header_tokens + tokens.retrieved_body_tokens
        );
        assert!((tokens.tokens_per_answer - tokens.answer_tokens as f64).abs() < f64::EPSILON);
    }

    #[test]
    fn validates_expected_matches() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "token").expect("write a");
        crate::index_corpus(root.as_path(), database.as_path()).expect("index");
        let cases = vec![EvaluationCase {
            name: "invalid".to_owned(),
            query: "query".to_owned(),
            changed_paths: Vec::new(),
            required_files: Vec::new(),
            expected: Vec::new(),
        }];

        assert!(matches!(
            evaluate(
                database.as_path(),
                cases.as_slice(),
                1,
                RetrievalStrategy::Substring
            ),
            Err(Error::EvaluationCaseWithoutExpected(name)) if name == "invalid"
        ));
    }

    #[test]
    fn required_files_define_relevance_without_text_fragments() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("target.rs"), "pub fn token() {}\n").expect("write target");
        crate::index_corpus(root.as_path(), database.as_path()).expect("index");

        let cases = vec![EvaluationCase {
            name: "fixed task".to_owned(),
            query: "token".to_owned(),
            changed_paths: Vec::new(),
            required_files: vec!["target.rs".to_owned()],
            expected: Vec::new(),
        }];

        let report = evaluate(
            database.as_path(),
            cases.as_slice(),
            1,
            RetrievalStrategy::Fts5Reranked,
        )
        .expect("evaluate");

        assert_eq!(report.hits, 1);
        assert_eq!(report.results[0].required_files, vec!["target.rs"]);
        assert_eq!(report.results[0].rank, Some(1));
    }

    #[test]
    fn baseline_reports_metrics_for_live_file_grep_read() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("index.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("target.rs"), "pub fn paired_contract() {}\n").expect("write target");
        fs::write(root.join("other.rs"), "pub fn unrelated() {}\n").expect("write other");
        crate::index_corpus(root.as_path(), database.as_path()).expect("index");

        let cases = vec![EvaluationCase {
            name: "paired task".to_owned(),
            query: "paired_contract".to_owned(),
            changed_paths: Vec::new(),
            required_files: vec!["target.rs".to_owned()],
            expected: Vec::new(),
        }];

        let report =
            evaluate_baseline(database.as_path(), cases.as_slice(), 5).expect("baseline eval");

        assert_eq!(report.name, "grep_read");
        assert_eq!(report.hits, 1);
        assert_eq!(report.hit_rate, 1.0);
        assert_eq!(report.mean_reciprocal_rank, 1.0);
        assert_eq!(report.results[0].required_files, vec!["target.rs"]);
        assert_eq!(report.token_usage.answered_queries, 1);
        assert!(report.token_usage.tokens_per_answer > 0.0);
    }

    #[test]
    fn resolve_under_root_rejects_paths_escaping_the_corpus() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("a.txt"), "inside").expect("write inside");
        fs::write(temp.path().join("secret.txt"), "outside").expect("write secret");

        assert!(resolve_under_root(&root, "a.txt").is_some());
        assert!(resolve_under_root(&root, "../secret.txt").is_none());
        assert!(resolve_under_root(&root, "/etc/hosts").is_none());
        assert!(resolve_under_root(&root, "missing.txt").is_none());
    }
}
