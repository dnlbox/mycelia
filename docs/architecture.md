# Mycelia v1 — Architecture

This document describes what exists in the engine today, the target architecture for v1, and the gap between them. Cross-reference [requirements.md](requirements.md) for the binding contract and [../ROADMAP.md](../ROADMAP.md) for the build sequence that closes the gap.

## What exists today

### Crate structure

Two crates in the Cargo workspace:

- **`crates/mycelia-core`** — synchronous, no async. Owns the indexer, storage layer, retrieval strategies, and calls graph. No Tokio; no protocol types. This is the library boundary: narrow traits, no CLI concerns.
- **`crates/mycelia-cli`** — the async edge. Owns CLI commands and the MCP server. Tokio and the MCP SDK live here and do not leak into core.

### Chunking and extractors

Tree-sitter structural chunking for five languages: Rust, TypeScript (`.ts`), TSX, Python, Ruby. Each top-level declaration plus its preceding doc comment becomes one chunk. All other files use a plain-text fallback (paragraph-split, max 2048 bytes per chunk).

Extractors are versioned strings: `tree-sitter-rust-v1`, `tree-sitter-typescript-v1`, `tree-sitter-tsx-v1`, `tree-sitter-python-v1`, `tree-sitter-ruby-v1`, `plain-text-v1`. When chunk boundaries change, the version bumps and stale chunks and their embeddings are invalidated (R4).

### Storage

SQLite, schema version 5. Tables: `metadata`, `sources`, `chunks`, `chunk_fts` (FTS5 virtual table), `embeddings`, `edges`. Project-local database path: `.mycelia/db/index.sqlite3`. Migrations are forward-only and numbered. FTS5 is kept synchronised via migration-owned triggers on `chunks`.

**Chunk IDs:** `BLAKE3(source_path + source_hash + byte_start:byte_end)`. Deterministic; indexing the same commit twice yields byte-identical IDs (R3).

### Embeddings

BAAI/bge-small-en-v1.5 via FastEmbed (ONNX), stored as little-endian f32 blobs in the `embeddings` table alongside model identity and vector dimensions. Embeddings are optional: the lexical-only path (`--no-embed` / `--lexical`) requires no model download and is the required CI path (R6).

### Retrieval strategies

Six strategies, all implemented:

| Strategy | Description |
| --- | --- |
| `substring` | Literal substring scan; reference adapter |
| `fts5` | Raw BM25 FTS5; reference adapter |
| `fts5_reranked` | BM25 with 20 candidates reranked by exact-phrase / token-coverage / signature-line coverage; lexical baseline and fallback |
| `vector` | Brute-force cosine similarity over stored embeddings |
| `hybrid` | Lexical + vector combined |
| `routed` | Query-class routing; selects lexical-first or semantic-first profile; falls back to `fts5_reranked` when no embeddings are present |

`routed` is the CLI and MCP default when embeddings are available. The synchronous core API defaults to `fts5_reranked`.

### Freshness model (R2)

`retrieve` validates the on-disk source hash before returning. Three outcomes: `Ok` (precise chunk, file unchanged), `File` (source drifted — returns live whole file), `Unavailable` (source gone or unreadable). `find` validates top-K headers against on-disk hashes, self-heals drift via `refresh_source`, and re-ranks once. A stale slice is never served.

### MCP surface (R5)

Stdio, read-only. Stdout is reserved for protocol messages; diagnostics go to stderr. Six tools exposed to the model:

| Tool | Description |
| --- | --- |
| `find` | Ranked headers only (path, byte/line range, score, `chunk_id`, signature or synopsis) — no chunk bodies |
| `search_codebase` | Alias for `find` |
| `locate_implementation` | Alias for `find` |
| `retrieve` | One chunk body by `corpus:chunk_id`, freshness-validated |
| `find_related` | Callers or callees of a symbol via the `calls` graph (Rust only today); ambiguous names returned with `resolved=false` |
| `list_corpora` | Registered corpus names and roots |

Model-facing headers trim `source_hash`, `extractor`, and byte offsets — the model sees only what it needs to act.

### Calls graph

Shipped for Rust only. Extracts free-function, path, and macro call edges by callee name. Stored in the `edges` table by `dst_symbol`; resolved at query time against the current symbol index. Method calls are deliberately omitted (receiver type is unknown; the bare name would misresolve). Conservative resolution: drop unresolved names; return all candidates for ambiguous names with `resolved=false`; never silently pick one.

---

## Target architecture for v1

### CI artifact model (Phase 1)

The core v1 invariant is that the index is materialised from one commit and is reproducible at that SHA. This requires:

- **`mycelia ci prepare`:** build-or-restore the index at the current commit, validate schema, emit the cache key (R8) and environment for downstream steps.
- **Artifact export/import/verify:** a manifest carrying `mycelia_version`, `schema_version`, `project_name`, `git_commit`, `source_root_hash`, `extractors`, `embedding_model`, and `db_files`. Import verifies every field before use and rejects any mismatch with a named reason (R7).
- **Cache key composition (R8):** Mycelia version + schema version + extractor versions + project-config hash + git commit. A near-miss `restore-keys` restores the previous commit's artifact for incremental refresh.
- **Git-diff-aware incremental refresh:** restore the previous artifact, then update only changed files. This is what makes warm CI runs fast (R10).

See [../ROADMAP.md](../ROADMAP.md) Phase 1 and requirements R7, R8, R10 for the gate criteria.

### Change-scoped retrieval (Phase 2)

Given a diff, return the blast radius: changed symbols plus their callers and callees via the `calls` graph. This requires extending the calls graph to TypeScript (Vercel-ecosystem priority) and Python, keeping the same conservative resolution rules. Exposed through `find_related` and `find`.

See [../ROADMAP.md](../ROADMAP.md) Phase 2.

### AI SDK integration (Phase 3)

The MCP server is already consumable via `@ai-sdk/mcp` `createMCPClient` in stdio mode. Phase 3 verifies this end-to-end and ships:

- A reference `review-agent.mjs` using `ToolLoopAgent`, AI Gateway model routing, Node 22 ESM, running headless.
- The reference GitHub Actions workflow (checkout, cache, `mycelia ci prepare`, agent).
- Optionally, a thin typed `@mycelia/ai-sdk` npm wrapper exporting `tool()` definitions, only after the MCP surface is re-audited for deterministic structured output.

All AI SDK integration targets version 7.0 exclusively. See [../AGENTS.md](../AGENTS.md) for the version guard.

---

## Gap list

The following are stubbed or absent today and are delivered by the roadmap phases listed:

| Gap | Roadmap phase |
| --- | --- |
| `mycelia ci prepare` command | Phase 1 |
| Artifact export / import / verify with manifest (R7) | Phase 1 |
| Cache-key composition and emission (R8) | Phase 1 |
| Git-diff-aware incremental refresh | Phase 1 |
| TypeScript `calls` graph edges | Phase 2 |
| Python `calls` graph edges | Phase 2 |
| Change-scoped retrieval from diff | Phase 2 |
| Reference `review-agent.mjs` (AI SDK 7.0) | Phase 3 |
| Reference GitHub Actions workflow | Phase 3 |
| Optional `@mycelia/ai-sdk` npm wrapper | Phase 3 |
| Public paired benchmark (PR-review bakeoff) | Phase 4 |
| `stats --all` | Not yet scheduled |
| Watcher / debounce | Out of scope for v1 (CI is ephemeral) |

Items not on this list are out of scope for v1. See [docs/vision.md](vision.md) "Explicitly out of scope."
