# Query-class routing

## Why now

Slice `12` confirmed that the remaining hybrid misses are intent-shaped queries:
"approve a run at a gate", "tenant lease context", "session continuity". Reranked
FTS5 recovers most exact symbol lookups; the flat hybrid applies the same
lexical/semantic blend to both query shapes, which means it either protects
lexical too aggressively (missing NL intent) or leans semantic (losing precision
on symbol lookups). The oracle analysis over the Round 2 fixture set predicted a
clear, non-overfit win if routing could direct each query to the right blend
before the merge step.

## Goal

Route queries to different hybrid profiles based on a lightweight, offline
query classifier:

- Symbol-like queries (identifiers, type names, function signatures) → lexical
  profile: strong FTS5 spine, vector expansion at lower weight.
- Natural-language intent queries → semantic profile: minimal lexical floor,
  high vector weight, no shortcut path.

Fail gracefully when embeddings are not present: fall back to reranked FTS5
rather than refusing the query.

## Locked decisions

- **No inference in the classifier.** Classification is a small, deterministic
  heuristic over stop-word count and identifier token count. Stop words are a
  frozen constant list (`QUERY_STOP_WORDS`). This costs no tokens and has no
  cold-start latency.
- **Three profiles, not one.** `HybridProfile` is a struct with three fields:
  `protected_lexical` (minimum lexical candidates shielded from displacement),
  `vector_weight` (multiplicative factor in the RRF merge), and
  `strong_lexical_shortcut` (early-exit threshold). The `balanced` profile is
  used by the existing `hybrid` strategy; `lexical` and `semantic` are used by
  `routed`. The explicit `hybrid` strategy keeps `balanced` so its behaviour is
  not silently changed.
- **`routed` is an explicit opt-in.** The default strategy remains
  `fts5-reranked`. Keeping it as the default avoids cold-start model downloads on
  embedding-free databases and does not break anything that relied on that
  default.
- **Fallback to lexical when embeddings are absent.** `find_routed` catches
  `Error::MissingEmbeddings` and retries with `fts5-reranked`. This means
  `--strategy routed` works safely against a non-embedded database.

## Classifier

```
classify_query(query: &str) -> QueryClass
```

Tokenises on whitespace and lowercases. Counts:
- `stop_count`: tokens appearing in `QUERY_STOP_WORDS`
- `identifier_count`: tokens matching `is_identifier_token` (contains `_`,
  `::`, camelCase, PascalCase, or ALLCAPS > 1 char)

Rules (in order):

| Condition | Class |
| --- | --- |
| `stop_count >= 2` | `NaturalLanguage` |
| `identifier_count >= 1 && stop_count == 0` | `SymbolLike` |
| `token_count >= 6 && stop_count >= 1` | `NaturalLanguage` |
| _(else)_ | `SymbolLike` |

## Profiles

| Profile | `protected_lexical` | `vector_weight` | `strong_lexical_shortcut` |
| --- | ---: | ---: | ---: |
| `balanced` (used by `hybrid`) | 3 | 2.0 | Some(2.0) |
| `lexical` (symbol-like) | 3 | 1.5 | Some(1.0) |
| `semantic` (natural-language) | 1 | 4.0 | None |

The shortcut threshold means: if the top FTS5 candidate's score exceeds the
threshold and the lexical list already fills `limit`, return it immediately
without vector expansion. `semantic` disables this so strong keyword hits cannot
suppress semantic candidates on intent queries.

## Measured result

Evaluation run over the Forge cross-repo corpus (5 repos, 7,411 chunks,
`BAAI/bge-small-en-v1.5` 384-dim embeddings):

| Strategy | Baseline (40) | Baseline MRR | Paraphrase (10) | Paraphrase MRR | Expanded (18) | Expanded MRR | Total (68) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Reranked FTS5 | — | — | — | — | — | — | 34 |
| Vector | — | — | — | — | — | — | 33 |
| Hybrid (balanced) | 24 / 40 | 0.476 | 0 / 10 | 0.000 | 13 / 18 | 0.374 | 37 |
| Routed | 24 / 40 | 0.479 | 3 / 10 | 0.070 | 14 / 18 | 0.419 | 41 |

Routed adds four hits over the flat balanced hybrid across the combined fixture
set: the paraphrase cases were entirely dark (0 / 10) under hybrid because its
lexical shortcut fired on exact-keyword heads and suppressed semantic expansion;
routing those queries to the semantic profile recovered three of them.

Oracle analysis before implementation confirmed the gain was not
dataset-overfitted: the classifier's stop-word heuristic correctly split the
fixture set with only two boundary cases, and the profile weights were not tuned
against the eval fixtures.

## Decision

Merge `routed` as a selectable strategy, not as the new default.

The paraphrase and expanded wins are real and directionally correct, but the
absolute numbers (3 / 10 paraphrase, 24 / 40 baseline held, not improved) show
the classifier heuristic is the right architecture and the wrong stopping point.
The next meaningful lift will come from typed edges on code queries (relationship-
shaped questions that no retrieval strategy can answer from flat chunks alone).

The semantic profile carries some risk: vector-only recall on intent queries
requires good embeddings, and the corpus still has queries that land between
"clear symbol" and "clear prose". A finer classifier (e.g., one that weighs
camelCase tokens against stop words jointly, or uses query length) is straightforward
but premature before the typed-edge layer shows what query classes actually remain.

## Deferred

- Promoting `routed` to the default strategy (blocked on typed-edge work showing
  whether remaining misses are routing failures or structural missing context).
- Smarter classifier: part-of-speech tags, query length normalisation, token
  overlap with corpus vocabulary.
- Per-corpus calibration of `strong_lexical_shortcut` based on observed FTS5
  score distributions.
- Embedding throughput: 547 s to embed 7,411 chunks on CPU is the active cost
  gate for any frequent re-index loop; this becomes urgent before typed-edge
  extraction adds more content.
