# Mycelia v1 — Requirements (the binding contract)

These are non-negotiable invariants. Every slice in [../ROADMAP.md](../ROADMAP.md) preserves them. Each carries a verifiable check.

## R1 — Two-stage retrieval contract
`find` returns ranked **headers only** (path, byte/line range, score, `chunk_id`, signature or synopsis) within a token budget. `retrieve` returns **one full chunk body** by `corpus:chunk_id`. Never collapse them into one fat call.
- **Check:** a `find` response carries no chunk bodies; `retrieve` returns exactly the requested chunk.
- **Rationale:** hit-rate gains that increase tokens-per-answer are not wins (see [evaluation.md](evaluation.md)).

## R2 — Freshness contract (freshness is trust)
Never serve a stale slice. `retrieve` returns one of: the precise chunk (fresh), the **live whole file** (chunk drifted), or `unavailable`. `find` validates the top-K against on-disk hashes, self-heals drift, and re-ranks once.
- **Check:** mutate a source file after indexing; `retrieve` of an affected chunk never returns stale bytes.

## R3 — Deterministic chunk IDs
`chunk_id = BLAKE3(source_path + source_hash + byte_start:byte_end)`. Indexing the same commit twice yields identical IDs.
- **Check:** build the index twice at the same SHA; the set of `(chunk_id)` is byte-identical.

## R4 — Extractor versioning invalidates correctly
Extractors are versioned strings (`tree-sitter-rust-v1`, …). When chunk boundaries change, the extractor version bumps; stale chunks **and their embeddings** are invalidated.
- **Check:** bumping an extractor version forces re-chunk + embedding invalidation for that language.

## R5 — Read-only MCP surface
No model-facing mutation tools. The model cannot pass arbitrary database paths. Surface is exactly: `find`, its aliases (`search_codebase`, `locate_implementation`), `retrieve`, `find_related`, `list_corpora`.
- **Check:** no MCP tool writes to the index or filesystem; no tool accepts a raw DB path argument.

## R6 — Lexical-only CI path
CI must run with **no embedding-model download**: `--no-embed` / `--lexical`. Retrieval falls back to `fts5_reranked` when no embeddings are present.
- **Check:** `mycelia ci prepare --no-embed` produces a working index and `find` returns ranked results with zero network access.

## R7 — CI artifact manifest
An exported index artifact carries: `mycelia_version`, `schema_version`, `project_name`, `git_commit`, `source_root_hash`, `extractors`, `embedding_model`, `db_files`. Import **verifies every field** before use.
- **Check:** import of an artifact with any mismatched field is rejected with a named reason.

## R8 — CI cache-key composition
The cache key is composed of: Mycelia version + schema version + extractor versions + project-config hash + git commit. A near-miss (`restore-keys`) restores the previous commit's index for incremental refresh.
- **Check:** changing any composing input changes the key; an unchanged tree reuses the artifact byte-for-byte.

## R9 — Observability names the fix
`status` reports chunk count, graph edges, embedding coverage, last refresh. `stats` reports token savings (`actual_tok` vs `cold_tok` per `find`). Both must state the corrective action when a value is degraded.
- **Check:** on a partially-indexed corpus, `status` reports the gap and the command that fixes it.

## R10 — Speed targets
Small repo: index in seconds. Medium repo: within the CI setup budget (cold), or restore-then-incremental (warm). `find` headers must be cheaper than grep/read loops in an agent.
- **Check:** measured against the eval task set; recorded per phase in the roadmap gates.

## R11 — Eval manifests excluded from discovery
Oracle queries / eval manifests never contaminate corpus retrieval.
- **Check:** indexing a repo containing an eval manifest does not surface manifest content in `find`.

## Engine constraints (carried from the working codebase)
- **No async in core**; the core crate stays synchronous. Async lives at the CLI/server edge.
- **Narrow traits** over broad ones.
- **Brute-force vector similarity is acceptable** until measured as the bottleneck — do not add an approximate index speculatively.
- **Conservative graph resolution:** drop unresolved names; return all candidates for an ambiguous name with `resolved=false`; never silently pick one. A wrong connection is worse than none.
