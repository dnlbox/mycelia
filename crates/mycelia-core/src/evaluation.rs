use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use crate::{
    EmbeddingProvider, Error, EvaluationCase, EvaluationCaseResult, EvaluationReport,
    ExpectedMatch, Result, RetrievalStrategy, SearchHeader, SearchHit, TokenUsageReport, store,
};

pub(crate) fn evaluate(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: RetrievalStrategy,
) -> Result<EvaluationReport> {
    let corpus_root = store::corpus_root(database)?;
    evaluate_cases(cases, limit, strategy, corpus_root.as_ref(), |query| {
        crate::find_with_strategy(database, query, limit, strategy)
    })
}

pub(crate) fn evaluate_with_embeddings(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: RetrievalStrategy,
    provider: &mut dyn EmbeddingProvider,
) -> Result<EvaluationReport> {
    let corpus_root = store::corpus_root(database)?;
    evaluate_cases(cases, limit, strategy, corpus_root.as_ref(), |query| {
        crate::find_with_embeddings(database, query, limit, strategy, provider)
    })
}

fn evaluate_cases<F>(
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

        let found = find(case.query.as_str())?;
        let rank = first_relevant_rank(found.as_slice(), expected.as_slice());
        if let Some(rank) = rank {
            hits += 1;
            reciprocal_rank_sum += 1.0 / rank as f64;
            token_usage.add_answer(found.as_slice(), rank - 1, corpus_root);
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
                required_files: Vec::new(),
                expected: vec![ExpectedMatch {
                    source_path: "b.txt".to_owned(),
                    contains: None,
                }],
            },
            EvaluationCase {
                name: "miss".to_owned(),
                query: "needle".to_owned(),
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
