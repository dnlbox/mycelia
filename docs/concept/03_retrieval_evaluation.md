# Retrieval evaluation

## Goal

Measure retrieval quality before changing the retrieval architecture. A candidate
implementation must improve named query cases without weakening provenance,
freshness, or deterministic behavior.

## Manifest

An evaluation manifest is JSON:

```json
{
  "limit": 5,
  "cases": [
    {
      "name": "find the subscription boundary",
      "query": "subscription-first",
      "expected": [
        {
          "source_path": "docs/concept/00_vision.md",
          "contains": "Subscription-first"
        }
      ]
    }
  ]
}
```

Each expected match names a source path and may require an exact text fragment in
the returned chunk. A case passes when any expected match appears within the
configured result limit.

## Metrics

- **Hit rate at limit**: passing cases divided by all cases
- **Mean reciprocal rank (MRR)**: mean of `1 / rank` for the first relevant result,
  with misses contributing zero

The evaluator also returns each case's first relevant rank. These metrics are
baseline signals, not proof of semantic quality. Manifests must represent real
questions and include failure-oriented cases before they guide architecture.

### Tokens per answered query (the product metric)

Hit rate and MRR are proxies. The measure that justifies the index is the tokens
a client spends to reach an answer versus reading the source cold. For
representative cases, record the tokens returned by a default `find` plus the
`retrieve` bodies actually needed, against the tokens to open the source file(s).
This metric gates the distilled surface (`10`) and the hybrid (`11`): a retrieval
change that does not lower tokens-per-answer is not a win for the code use case,
whatever it does to hit rate.

## CLI

```text
mycelia eval <manifest> --database <path> [--json]
```

The evaluator reads an existing index. It never mutates the corpus or database.

## Decision gate

Do not add embeddings, full-text search, reranking, or graph traversal because a
technology is available. Add a candidate behind the retrieval boundary, run the
same manifests, and compare quality, latency, index cost, and storage.
