# Lightweight FTS5 reranking

## Goal

Recover the four known FTS5 top-five regressions without losing any of FTS5's 16
hits on the unchanged Forge manifest.

## Candidate

Retrieve a wider BM25 candidate set, then rerank deterministically by:

1. normalized exact phrase match
2. distinct query-token coverage
3. signature-line coverage for identifier-shaped query terms
4. BM25 score
5. source path, byte start, and chunk id

Normalization uses the same alphanumeric and underscore token boundaries as the
safe FTS query builder. This rewards a chunk containing the complete user phrase
across Markdown punctuation or line breaks, without adding a second parser or a
learned model.

Limited ranked headers collapse exact duplicate chunk bodies after scoring. The
first ranked copy is retained and the next distinct candidate fills the result
budget, which prevents cloned boilerplate from hiding useful nearby answers.

## Strategy

Expose the candidate as `fts5-reranked`. Keep raw `fts5` and `substring` available
for evaluation.

## Decision gate

Promote the reranker only if it retains all 16 FTS5 hits and recovers at least one
regression on the unchanged 20-case manifest. Record any new regressions.

## Measured result

| Strategy | Hits | Hit rate | MRR | Wall time |
| --- | ---: | ---: | ---: | ---: |
| Substring | 11 / 20 | 0.55 | 0.308 | 0.02s |
| FTS5 + BM25 | 16 / 20 | 0.80 | 0.541 | 0.01s |
| FTS5 + reranking | 20 / 20 | 1.00 | 0.688 | 0.02s |

The reranker recovered all four raw FTS5 regressions and introduced none on this
manifest. It is now the default. Raw FTS5 and substring remain selectable reference
strategies.

This result is strong but narrow. The next evaluation set must include more code
queries, ambiguous terms, expected misses, and a larger corpus before the ranking
weights are treated as stable.

2026-06-27 follow-up: exact duplicate-body suppression improved the current
refreshed Forge lexical comparison from 47/68 to 48/68 and routed from 51/68 to
52/68. The shipped change is header compaction, not a new weighting signal; a
tested source-path token boost was rejected because it traded away established
hits and worsened tokens per answer.
