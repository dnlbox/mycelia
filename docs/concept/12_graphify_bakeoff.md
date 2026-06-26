# Graphify bakeoff

## Why now

Slice `11` proved that flat lexical/vector hybrid tuning is not enough. The
lexical-spine hybrid recovered a few cases without losing baseline hits, but it
failed the token-per-answer gate on the 40-case Forge baseline. Before adding
typed edges ourselves, compare against Graphify because it already ships a local
AST graph, graph traversal queries, MCP support, and assistant integration.

## Scope

This was a local, no-cost bakeoff. No external model calls were made.

Graphify can extract code locally, but its full extraction path sends docs,
papers, images, and other non-code content through a configured AI backend. No
model-provider credentials were configured in the bakeoff environment, and
cost-bearing model calls require explicit approval. Therefore this slice
benchmarked Graphify's local code graph against Mycelia on the same code-only
Forge mirror.

The code-only mirror preserved Forge-relative paths and included Rust files plus
Cargo manifests:

```text
33 files
mycelia/...
bifrost/...
```

## Commands

```text
graphify extract <tmp>/forge-code-corpus --out <tmp>/graphify-bakeoff-clustered
mycelia index <tmp>/forge-code-corpus --database <tmp>/mycelia-code-bakeoff.sqlite3
mycelia eval fixtures/eval/forge-structural.json --database <tmp>/mycelia-code-bakeoff.sqlite3 --strategy fts5-reranked --json
graphify query "<case query>" --graph <tmp>/graphify-bakeoff-clustered/graphify-out/graph.json --budget 2000
```

## Results

| System                   |                                          Build result |          Structural hits |                                 Token proxy |            Storage |
| ------------------------ | ----------------------------------------------------: | -----------------------: | ------------------------------------------: | -----------------: |
| Mycelia reranked FTS5    |                           33 files, 861 chunks, 63 ms |                    7 / 8 |   810 tokens per answered query, 9,130 cold |     964 KiB SQLite |
| Graphify local AST graph | 24 code files, 464 nodes, 1,429 edges, 12 communities | 6 / 8 source+symbol hits | 1,174 tokens per query, about 1,549 per hit | 627 KiB graph.json |

Graphify's built-in benchmark on the clustered code graph reported:

```text
Corpus:          23,200 words -> ~30,933 tokens naive
Graph:           464 nodes, 1,429 edges
Avg query cost:  ~8,588 tokens
Reduction:       3.6x fewer tokens per query
```

The built-in benchmark is not the same metric as Mycelia's sourced retrieval
evaluator, but it confirms that Graphify's graph neighborhoods are materially
larger than Mycelia's header-plus-retrieve path for the structural cohort.

## Case Notes

Graphify is strong when the query names a symbol exactly. For example,
`graphify explain "chunk_record_from_row"` returns the correct source location
and extracted references/calls:

```text
Source: <tmp>/forge-code-corpus/mycelia/crates/mycelia-core/src/store.rs L628
Connections: Result, store.rs, ChunkRecord, find_fts5_candidates(), load_embeddings(), row_i64_to_usize(), Row
```

The misses were natural-language intent questions where the relevant symbol was
present in the graph but the query planner picked the wrong seed:

1. "function that reads a row from rusqlite and returns a ChunkRecord with span and extractor"
   started from `ChunkRecord`, `Row`, and `From`, then missed `chunk_record_from_row()`.
2. "return the extractor identifier for a source path based on its file extension"
   started from `Path`, then missed `extractor_id_for()`.

This is the same broad failure class Mycelia hit before code-aware chunking and
hybrid work: a good index is not enough if query-to-unit routing is weak.

## Decision

Do not stop Mycelia and simply adopt Graphify based on this local measurement.
Graphify is more mature as an AST graph product and should remain a reference,
but its local code query path did not beat Mycelia on the current structural
evaluation. Mycelia's default retrieval was more accurate and cheaper on the same
code-only corpus.

Do not dismiss Graphify either. Its graph model has real advantages Mycelia has
not built yet: extracted calls, imports, contains edges, `explain`, `path`,
affected traversal, MCP serving, watch/update flows, and assistant install hooks.

## Next Step

Run a full-corpus Graphify comparison only behind an explicit model-spend gate.
That comparison should measure:

1. baseline, paraphrase, and structural source hits,
2. output tokens per answered query,
3. model input/output tokens and backend cost,
4. provenance quality,
5. refresh behavior,
6. whether Graphify's semantic document extraction improves the paraphrase
   cohort enough to justify the external-call boundary.

If the full Graphify run wins on paraphrase and source provenance without an
unacceptable cost or privacy tradeoff, wrap or adopt it for graph extraction
rather than rebuilding that layer. If it does not, borrow its AST edge model and
focus Mycelia's next slice on query-class-aware routing or typed edges for
intent-shaped code questions.

---

# Round 2: full corpus, full extraction

This is the model-spend-gated run the section above called for. Two more Forge
repos were added to widen the indexer's use cases, the corpus grew to cover four
languages plus docs, and Graphify was set up completely with a real backend so
its semantic document extraction actually ran (no longer code-only).

## Setup

- **Graphify install:** PyPI package is `graphifyy` (the `graphify` name is being
  reclaimed). The LLM backend needs the extra: `uv tool install
  "graphifyy[anthropic]" --force`. Without it, semantic chunks fail with "the
  'anthropic' package is required".
- **Backend / credentials:** `--backend claude`, with credentials supplied
  through the process environment. This is the external-call boundary the prior
  slice deferred.

## Corpus

A clean mirror of git-tracked, first-party files (code + docs + manifests) from
five Forge repos, preserving Forge-relative paths (vendored venvs excluded):

```text
357 files, 2.5 MiB
mycelia/             36   Rust
bifrost/             32   Rust
ai-protocol/         38   pure markdown (concept docs)
candelabrum-studio/ 104   TypeScript / React (ts, tsx, md)
operio-agent/       147   Python + TypeScript + markdown
---
by extension: 160 md, 96 ts, 43 py, 25 rs, 22 tsx, 7 toml, 3 json, 1 txt
```

A new 18-case cross-repo structural fixture covers all five repos with a mix of
exact-symbol and natural-language intent queries:
`fixtures/eval/forge-expanded.json`.

## Commands

```text
graphify extract <tmp>/forge-corpus-full --backend claude --out <tmp>/graphify-forge-v2
graphify benchmark <tmp>/graphify-forge-v2/graphify-out/graph.json
graphify query "<case query>" --graph .../graph.json --budget 2000
mycelia index  <tmp>/forge-corpus-full --database <tmp>/mycelia-forge-v2.sqlite3
mycelia embed  --database <tmp>/mycelia-forge-v2.sqlite3
mycelia eval   fixtures/eval/forge-expanded.json --database ... --strategy <s> --json
```

## Results (18 cross-repo cases)

Mycelia is scored as ranked top-5 (its real output). Graphify returns a BFS
neighborhood meant to be fed to an LLM, so it is scored two ways: the answer node
ranked inside the top-5 nodes (apples-to-apples), and the answer node present
anywhere in the budget-capped neighborhood (its own fair metric).

| System / strategy             |                Hits |   MRR | Tokens/query |   Build cost |          Build time |
| ----------------------------- | ------------------: | ----: | -----------: | -----------: | ------------------: |
| Mycelia hybrid (top-5)        |    **14/18 (0.778)** | 0.552 |       ~606 |   local/free | index 0.2 s + embed 547 s |
| Mycelia vector (top-5)        |          13/18 (0.722) | 0.502 |       ~606 |   local/free |        (embed reused) |
| Mycelia fts5-reranked (top-5) |          10/18 (0.556) | 0.338 |       ~606 |   local/free |     index 0.2 s, no embed |
| Graphify query, anywhere      |               11/18 |     - |     ~1,247 | $1.49 (LLM)  |       ~2.5 min extract |
| Graphify query, top-5 nodes   |                3/18 |     - |     ~1,247 | $1.49 (LLM)  |       ~2.5 min extract |

Graphify's build: 1,736 nodes, 3,227 edges, 141 communities; 231,006 input /
53,171 output tokens; its built-in benchmark reported **90.6x** token reduction
(~1,277 tokens/query) on this corpus, up from 3.6x on the old code-only mirror.

## What the numbers say

1. **Hybrid retrieval scales to the bigger, mixed corpus.** On a ~10x larger,
   four-language corpus, flat fts5 precision fell (the prior code-only run was
   7/8; here fts5 is 10/18). Adding the local embedding/vector signal recovered
   it: hybrid reaches 14/18 with MRR 0.55, and does so at ~606 tokens/answer.
   This is the strongest evidence yet for Mycelia's embeddings-centric thesis.

2. **Mycelia beats Graphify's local query path on accuracy, ranking, and cost.**
   14/18 ranked top-5 vs Graphify's best of 11/18 only-if-you-read-the-whole-
   neighborhood (and just 3/18 actually ranked in its top-5). Mycelia uses about
   half the tokens per query and has zero marginal cost; Graphify's run cost
   $1.49 and crossed the external-call boundary.

3. **Graphify's graph is good; its query routing is the bottleneck.** The
   $1.49 doc extraction worked: it produced exactly the right concept nodes
   (`Concept: Project Blueprint`, `Session Continuity Protocol`,
   `Model Tier Routing`). But the BFS query planner seeds on a literal keyword
   match against node labels, so "self-organizing agentic workspace blueprint
   vision" seeded on `['Self']` and never reached the doc — all three
   ai-protocol doc cases missed. Seeded with the literal term ("Project
   Blueprint"), the same graph returns the answer at rank 1 in 16 nodes. This is
   the identical failure class flagged in Round 1, now confirmed at scale: a good
   index is wasted if query-to-unit routing is weak.

4. **Did semantic doc extraction justify the external call? Not here.** The
   paraphrase/doc cohort is exactly where Graphify's LLM extraction was supposed
   to win, and it scored 0/3 on the doc cases via its own query path, while
   Mycelia hybrid got 2/3. The extracted concept nodes are also low-degree
   (degree 1, only `references` edges back to their source file), so BFS gains
   little from them on this corpus.

## Caveats (fairness to Graphify)

This eval measures sourced structural retrieval, which favours Mycelia. It does
not measure Graphify's actual strengths: relationship traversal
(`what connects X to Y`), `explain`/`path`/`affected`, god-node discovery,
cross-repo graph merge, MCP serving, and watch/commit-hook refresh. An agent
that picks good seeds (via `explain` on a known node) gets far more from the
graph than the automated NL-query path shows. Graphify remains the more mature
graph product; it just is not a better retriever on these queries.

## Decision

**Continue developing Mycelia.** The expanded, model-spend-gated run did not
change the Round 1 conclusion; it strengthened it. Hybrid retrieval scaled to a
multi-language corpus with docs (14/18, MRR 0.55) and beat Graphify's local
query path on hit rate, ranking, and tokens, at zero marginal cost and without
leaving the machine. Graphify's full semantic extraction did not win the
paraphrase/doc cohort it was meant to win.

Keep Graphify as a reference for the graph layer Mycelia has not built (typed
EXTRACTED/INFERRED edges, traversal, `explain`/`path`/`affected`, cross-repo
merge), not as a dependency.

## Next slice

1. **Query-class-aware routing.** The remaining hybrid misses are all
   intent-shaped ("approve a run at a gate", "tenant lease context",
   "session continuity") — route natural-language intent differently from
   exact-symbol lookups. This is now the clearest ceiling.
2. **Typed edges, borrowed from Graphify's AST model**, for relationship-shaped
   questions Mycelia cannot answer at all today.
3. **Embedding throughput.** 547 s to embed 7,411 chunks on CPU is a real cost
   for the index/refresh loop; investigate batching or a faster local model
   before the corpus grows further.
