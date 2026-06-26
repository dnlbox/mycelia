# First milestone: a trustworthy local index

## Goal

Prove the smallest complete `mycelia` path against real local files:

`discover → extract → store → find`

The milestone indexes UTF-8 text into range-addressed chunks, persists them in
SQLite, skips unchanged files by content hash, removes stale records, and returns
ranked lexical matches with exact source spans. It establishes the contracts that
later embeddings, typed edges, tree-sitter extractors, and MCP tools must preserve.

## User-visible surface

The first executable is a local CLI:

```text
mycelia index <root> --database <path>
mycelia find <query> --database <path> [--limit <count>]
```

`index` reports discovered, indexed, unchanged, removed, and rejected file counts.
`find` prints ranked chunks with their path, byte range, line range, score, and
text. JSON output is available for tests and future integrations.

## Locked decisions

- Rust workspace with a reusable `mycelia-core` library and a thin `mycelia` CLI
- SQLite is metadata storage for the first milestone
- Chunk identifiers derive from source identity, source content hash, and range
- Discovery respects standard ignore files and excludes VCS metadata
- The first extractor accepts UTF-8 text and chunks on paragraph boundaries with
  deterministic size limits
- Lexical ranking is a baseline adapter, not the final retrieval architecture
- Database schema changes use forward-only numbered migrations
- Core behavior remains synchronous until measured concurrency requirements exist
- One database belongs to one canonical corpus root
- Byte ranges are zero-based and half-open; line ranges are one-based and inclusive
- Paths and text must be valid UTF-8 in this milestone
- Hidden files are eligible for indexing, VCS metadata is excluded, repository
  ignore files apply, and global Git ignore rules do not
- Symlinks are not followed
- A rejected changed source loses its old chunks so retrieval never serves stale
  content

## Explicit non-goals

- Embeddings or approximate nearest-neighbor indexes
- Typed or inferred graph edges
- Tree-sitter and binary document extraction
- File watching
- MCP
- Cross-corpus federation
- Encryption at rest
- Custom packed storage, `mmap`, quantization, SIMD, HNSW, or IVF-PQ

## Validation

The slice is complete when:

1. Formatting and Clippy pass with warnings denied.
2. Unit and integration tests pass.
3. A release build succeeds.
4. A CLI smoke test indexes a fixture corpus twice, demonstrates unchanged-file
   skipping, changes one file, removes one file, and returns a sourced result.

## Decisions deferred by evidence

Representative corpora must determine chunk sizing, extractor concurrency,
embedding backends, vector indexing, and storage layout. The first milestone
records enough timing and count information to support those decisions later.
