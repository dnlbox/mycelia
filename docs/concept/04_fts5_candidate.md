# FTS5 retrieval candidate

## Goal

Test whether tokenized SQLite FTS5 retrieval with BM25 improves the measured
20-case Forge baseline before introducing embeddings.

## Design

- Keep substring retrieval as the reference adapter
- Add an explicit retrieval strategy to `find` and `eval`
- Store an external-content FTS5 index over `chunks.text`
- Maintain the index with insert, update, and delete triggers on `chunks`
- Backfill existing databases with the FTS5 `rebuild` command in migration 002
- Use the `unicode61` tokenizer with diacritic removal
- Convert user input to safely quoted token OR expressions
- Order by SQLite's BM25-backed `rank`, then deterministic source and span ties

The token OR query is deliberate. It supports reordered terms and punctuation while
BM25 rewards chunks that contain more of the query. It is a retrieval candidate,
not a user-facing advanced FTS query language.

## CLI

```text
mycelia find <query> --strategy substring|fts5 --database <path>
mycelia eval <manifest> --strategy substring|fts5 --database <path>
```

The measured comparison promoted FTS5 over substring. A later deterministic
reranker became the default while raw `fts5` and `substring` remain available as
reference adapters and diagnostic fallbacks.

## Decision gate

Run both strategies against the same 20-case manifest. Promote FTS5 only if it
materially improves hit rate or mean reciprocal rank without breaking freshness,
provenance, migration safety, or deterministic ordering.

## Measured result

Against the same Forge manifest at limit 5:

| Strategy | Hits | Hit rate | MRR | Wall time |
| --- | ---: | ---: | ---: | ---: |
| Substring | 11 / 20 | 0.55 | 0.308 | 0.02s |
| FTS5 + BM25 | 16 / 20 | 0.80 | 0.541 | < 0.01s |

FTS5 recovered reordered terms, punctuation, Markdown formatting, and line-break
queries. Four cases that substring found fell below FTS5's top five because broad
OR matching ranked legacy documents or source code higher. These regressions remain
in the manifest. Future ranking work must improve them without losing FTS5's gains.
