# Precision-first hybrid

## Why now

The probe (`08`) showed the shape of a working hybrid and the shape of a broken
one. Equal-weight reciprocal-rank fusion lost thirteen baseline hits while
recovering two paraphrases: it let weak semantic matches displace strong lexical
ones. Reranked FTS5 stayed the default for good reason.

This slice comes last because it depends on `09` and `10`. Hybrid weights tuned
over byte-window chunks would not survive structural chunking, and the
token-efficiency metric from `10` is the gate that tells us whether recovered
recall is worth its cost.

## Goal

1. Re-measure the embedding candidate over the clean symbol chunks from `09`,
   because the probe's weak vector recall conflated model quality with boundary
   noise.
2. Add one precision-first hybrid that uses the lexical result as the spine and
   semantics only to expand and reorder within it, never to displace top lexical
   hits.

## Locked decisions

- Lexical (reranked FTS5) provides the candidate spine. Its top results are
  preserved; semantic similarity may reorder lower positions and add candidates
  the lexical set missed, but may not evict an established top hit.
- Re-measure the embedding model on clean chunks before tuning the hybrid. Keep
  `BAAI/bge-small-en-v1.5` as the reference. The corpus contains non-English
  content (CJK in the `ai-protocol` benchmark dataset), so if semantic recall on
  non-English or code-intent queries is the bottleneck after clean chunking,
  evaluate a size-matched multilingual model (`intfloat/multilingual-e5-small`,
  also 384-dim) or a code-aware model before changing the storage layout.
- Keep brute-force cosine until corpus measurements justify an approximate index.
  HNSW, IVF, and quantization remain deferred.
- Raw `fts5`, `fts5-reranked`, `vector`, and the new hybrid stay independently
  selectable for evaluation. Promotion changes only the default.

## Decision gate

Promote the hybrid as default only if it:

1. recovers at least two paraphrase or structural cases over reranked FTS5,
2. loses no established baseline hit (stricter than `08`, which allowed one,
   because the spine is meant to protect precision),
3. improves or holds tokens-per-answered-query from `10`, and
4. preserves deterministic tie ordering and exact provenance.

If no hybrid clears this bar, keep reranked FTS5 as default and retain the
adapters and the recorded evidence.

## Measured result

The slice replaced the equal-weight RRF hybrid with a lexical-spine hybrid:

1. reranked FTS5 supplies the candidate spine,
2. a full page with a strong lexical top score returns unchanged,
3. otherwise the top three lexical hits are protected,
4. vector similarity may reorder lower candidates and add semantic expansions.

The re-measurement used FastEmbed 5.17.2 with
`BAAI/bge-small-en-v1.5`, 384 dimensions. On the fresh Forge index, 153 files
were discovered, 152 indexed, 1 rejected, and 3,264 chunks written with 0 code
parse fallbacks. The initial embedding build wrote 3,264 vectors in 231,346 ms
and stored 5,013,504 vector bytes. An unchanged refresh took 15 ms.

| Strategy | Baseline | Baseline MRR | Baseline tokens/answer | Paraphrase | Paraphrase MRR | Paraphrase tokens/answer | Structural | Structural MRR | Structural tokens/answer |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Reranked FTS5 | 35 / 40 | 0.721 | 1,014 | 1 / 10 | 0.033 | 590 | 5 / 8 | 0.563 | 775 |
| Vector | 18 / 40 | 0.411 | 590 | 3 / 10 | 0.225 | 610 | 8 / 8 | 1.000 | 797 |
| Lexical-spine hybrid | 36 / 40 | 0.727 | 1,061 | 2 / 10 | 0.053 | 584 | 6 / 8 | 0.594 | 764 |

## Decision

Keep the lexical-spine hybrid as a selectable measured adapter, but do not
promote it as the default. It recovered two paraphrase or structural cases over
reranked FTS5 and added one baseline hit without losing established baseline
hits, but it failed the token gate on the 40-case baseline: 1,061 tokens per
answered query versus 1,014 for reranked FTS5.

Vector retrieval is now clearly useful for code-intent structural queries, but
raw vector search still loses too much established baseline precision. The next
retrieval-quality work should not be more weight tuning in the same flat ranked
list. The evidence points toward typed edges or query-class-aware routing so
semantic expansion is used for intent-shaped code questions without charging
every exact lexical query.

## Deferred

- Approximate vector indexes and quantized storage.
- Learned rerankers and cross-encoders.
- Typed edges and graph traversal, which remain the next thesis-bearing slice
  after retrieval quality is settled.
