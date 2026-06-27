# Typed graph edges (first vertical: Rust `calls`)

Implemented 2026-06-27. This document records the first slice of the typed-edge
layer the architecture (`01_architecture.md`) always specified: chunks carrying
typed, sourced, confidence-tagged edges.

## Why now

The Graphify bakeoff (`12`) concluded by reserving this as the layer Mycelia had
not built: "typed EXTRACTED/INFERRED edges, traversal, explain/path/affected".
Relationship questions ("who calls X", "what does X depend on") are ones the
retrieval surface cannot answer at all. Prior miss analysis established that
edges will not move the token-per-answer retrieval gate, so this slice is judged
on a different axis: a new, sourced, conservative capability. It delivers a
depth-1 `calls` graph for Rust, exposed through the CLI and one MCP tool.

## North star constraint

`00_vision.md`: "a wrong connection is worse than none." Every edge is sourced
(it carries its call-site span) and confidence-tagged. The slice honours this
literally:

- A callee name with no in-corpus definition (a std or external call) resolves
  to nothing and is dropped in both relationship directions, never shown as a
  relationship.
- A name with several definitions is returned with every candidate and
  `resolved = false`, never silently collapsed to one.
- Method calls (`receiver.method()`) are not captured at all. The receiver type
  is unknown, so the bare method name would misresolve to an unrelated free
  function of the same name (the live `row.get()` to `profile::get` case caught
  in verification). Method edges need type resolution and are deferred.

## Design

### Storage (`migrations/005_graph_edges.sql`)

- `chunks.symbol` (nullable, indexed): the defined symbol name for a code chunk;
  `NULL` for plain text and for chunks indexed before this migration.
- `edges(src_chunk_id REFERENCES chunks(id) ON DELETE CASCADE, edge_type,
  dst_symbol, confidence, byte_start, byte_end, line_start, line_end)`: edges are
  stored by callee **name** (`dst_symbol`), not a resolved target id, with the
  call-site span as provenance.

### Query-time resolution

Resolution (`dst_symbol` -> defining chunk) happens at query time against the
current symbol index, never at write time. Rationale: indexing is incremental
and per-source, so a callee's defining file may be indexed later or re-indexed on
drift. Query-time resolution stays correct under partial reindex and matches the
freshness philosophy (answer against current state). `confidence` records only
the extraction-time class (`EXTRACTED` for deterministic tree-sitter edges);
ambiguity is computed at resolution, not stored.

### Automatic invalidation

`replace_source` deletes a source's chunks then re-inserts them; the `ON DELETE
CASCADE` on `src_chunk_id` purges that chunk's outgoing edges for free on every
reindex or prune. Edges live and die with their owning chunk, no extra
bookkeeping.

### Extraction (Rust only, `calls` only)

`extract_rust` populates `symbol` (the item's name; `impl` blocks key on the bare
type) and walks each top-level item's subtree for `call_expression` and
`macro_invocation` sites. It captures free-function calls (`foo()`), path calls
(`module::foo()`, final segment), and macro invocations; it drops method calls.
Chunk boundaries are unchanged, so the extractor identifier is **not** bumped and
embeddings stay valid (the spec bumps it only when boundaries change).

### Surface

- Core: `find_relationships(database, symbol, Direction)` returns sourced
  `RelatedHit`s (related definition header + call site + `resolved` +
  `definition_count`).
- CLI: `mycelia graph <symbol> [--direction callers|callees]`, read-only.
- MCP: a single parameterized `find_related(symbol, direction, corpus?)` tool.
  One tool, not several, per the `22` lesson that tool count feeds Claude Code's
  tool-search deferral. `edge_type` is reserved internally for future
  `imports`/`implements` without multiplying tools.
- `status` reports graph coverage (edges over symbols) and names `refresh` when a
  corpus predates the graph.

### Upgrade path

Two correctness fixes make existing corpora work:

1. `refresh` now performs a genuine forced full re-index (`reindex_corpus`),
   matching its own help text, so the graph backfills onto an unchanged corpus.
2. A read open transparently upgrades an out-of-date schema in place, so an
   existing v4 corpus does not fail `find` on the new `symbol` column. This
   upgrades an existing corpus; it never creates or repopulates one. It is
   consistent with the established decision that the server maintains its own
   bound index as internal upkeep.

The graph is empty (schema present, unpopulated) until the next `refresh`;
`status` says so.

## Verification

- Edge precision is asserted directly: extraction tests check exact caller/callee
  sets, zero false edges from comments/strings, method calls excluded, and
  ambiguity flagged. Store and CLI tests cover cross-file resolution, cascade
  purge, and the forced-reindex backfill.
- Live (release binary, self-indexed repo): `graph chunk_record_from_row
  --direction callers` returns `find_fts5_candidates` (the bakeoff example) and
  `relationship_callers`, both sourced; `--direction callees` returns
  `row_i64_to_usize` at its real call sites with no false `get` edge.
- Release-binary fixture smoke confirms external/no-def names such as `println`
  are omitted from callers as well as callees. A real stdio MCP exchange lists
  `find_related`, calls it, and exits cleanly.
- Retrieval gate on the refreshed 2026-06-27 Forge corpus: routed 50/68
  (weighted 1391.9 tokens per answered query) versus fts5-reranked 48/68
  (weighted 1395.9). The graph schema remains additive and does not justify a
  retrieval default change.

## Decision

Ship the conservative depth-1 Rust `calls` graph. Accuracy over coverage: drop
method and external calls rather than risk a wrong edge. Reserve for follow-ups:
edges for TS/Python/Ruby; `imports`/`implements`/`derived_from`; method-call
resolution via type information; INFERRED (cheap-model) edges; cross-corpus
edges; traversal beyond depth-1 (`path`/`affected`/`explain`).
