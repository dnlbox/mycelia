# First semantic embedding probe

## Why now

Lexical retrieval has reached its evidence boundary. Reranked FTS5 returns 35 of
40 Forge cases, exact code-symbol queries already pass, and further lexical fusion
regressed the aggregate. The remaining failures include semantic paraphrases that
cannot be solved reliably by token weighting.

The next substantive slice tests whether local embeddings improve those cases.
It does not commit Mycelia to a production vector index, embedding model, or
hybrid ranking formula.

Configuring a real MCP client remains a short acceptance check for the completed
transport work. It is not a separate implementation slice and does not block the
semantic probe.

## Goal

Add one local embedding candidate behind a narrow core interface and compare:

1. reranked FTS5
2. vector similarity
3. one explicitly defined lexical-vector hybrid

All candidates run against the same sourced evaluation manifests.

## Evaluation set

- Preserve the current 40-case Forge manifest unchanged.
- Classify the five current misses before implementation. Do not assume every
  miss is semantic.
- Add a small paraphrase-focused cohort with queries that deliberately avoid the
  expected source's exact vocabulary.
- Include negative or ambiguous cases so semantic recall cannot hide precision
  loss.

## Probe constraints

- Use a local, offline-capable embedding model. Corpus text must not leave the
  machine.
- Record the model identifier and vector dimensions with every cached embedding.
- Keep the embedding backend behind a narrow trait.
- Keep brute-force cosine similarity over the current roughly 3,200 chunks.
  HNSW, IVF, quantization, SIMD kernels, and specialized vector storage are out of
  scope until measurements require them.
- Preserve exact chunk identifiers and source provenance in every result.
- Reuse embeddings for unchanged chunks and invalidate them when the chunk or
  model identifier changes.
- Keep raw lexical and vector strategies independently selectable.
- Do not add tree-sitter, typed edges, federation, watchers, or MCP mutation tools
  in this slice.

## Measurements

Record for each strategy:

- hit rate and mean reciprocal rank on the original 40 cases
- hit rate and mean reciprocal rank on the paraphrase cohort
- newly recovered cases and newly introduced regressions
- initial embedding time and unchanged-corpus refresh time
- query latency
- embedding storage size
- model identifier, dimensions, and runtime/backend

## Decision gate

Promote no semantic strategy by default unless it:

1. recovers at least two currently missed or new paraphrase cases,
2. loses no more than one established original-manifest hit,
3. preserves deterministic tie ordering and exact provenance, and
4. remains operationally reasonable on the current Forge corpus.

If vector retrieval improves semantic recall but weakens established precision,
keep it as a measured adapter and use the results to design a later hybrid slice.
If it produces no material gain, remove the candidate and retain the recorded
evidence.

## Measured result

The probe used FastEmbed 5.17.2 with the 384-dimensional
`BAAI/bge-small-en-v1.5` model. FastEmbed resolved the ONNX artifact from
`Xenova/bge-small-en-v1.5`. Embeddings were stored as little-endian `f32` values
in SQLite and searched with brute-force cosine similarity.

On the unchanged 40-case Forge manifest at limit 5:

| Strategy | Hits | Hit rate | MRR | Evaluation time |
| --- | ---: | ---: | ---: | ---: |
| Reranked FTS5 | 34 / 40 | 0.850 | 0.667 | 70 ms |
| Vector | 21 / 40 | 0.525 | 0.443 | 1,219 ms |
| Equal-weight RRF hybrid | 31 / 40 | 0.775 | 0.527 | 1,301 ms |

On the 10-case paraphrase cohort:

| Strategy | Hits | Hit rate | MRR | Evaluation time |
| --- | ---: | ---: | ---: | ---: |
| Reranked FTS5 | 1 / 10 | 0.100 | 0.050 | 53 ms |
| Vector | 3 / 10 | 0.300 | 0.220 | 347 ms |
| Equal-weight RRF hybrid | 3 / 10 | 0.300 | 0.153 | 390 ms |

The initial 3,608-chunk embedding build took 238,288 ms and stored 5,541,888
vector bytes. An unchanged refresh took 11 ms. The final 3,629-chunk cache stored
5,574,144 vector bytes; incremental refreshes remained proportional to changed
chunks.

## Decision

Keep vector retrieval as a measured adapter, but do not promote it. It recovered
two additional paraphrase cases and mapped two hard code-intent queries well, but
lost thirteen established Forge hits. The equal-weight RRF hybrid also failed the
precision gate, losing three established hits relative to reranked FTS5.

Reranked FTS5 remains the default. The evidence supports a later precision-first
hybrid design, likely lexical candidate preservation with semantic expansion or
reranking, rather than replacing lexical retrieval or adding an approximate
vector index.
