# Indexer speed and artifacts

## Motivation

Prior-art review of `codebase-memory-mcp` exposed a useful bar: mature users
expect an indexer to feel cheap enough that adoption friction disappears. Their
implementation emphasizes a static binary, vendored parsers, fast graph builds,
bulk storage, and aggressive agent integration.

Mycelia should not copy the invasive user-level install behavior, but it should
borrow the speed and artifact lessons.

## Speed goals

Initial targets for v2:

- small repo: index in seconds,
- medium repo: index within CI setup budget,
- large monorepo: restore artifact/cache first, then incrementally refresh,
- query path: keep `find` header retrieval fast enough to beat grep/read
  exploration in agent loops.

Do not optimize blindly. Add measurements before storage rewrites.

## Borrowed implementation ideas

Investigate:

- parallel discovery and parsing with deterministic final ordering,
- bulk SQLite insert transactions,
- tuned SQLite pragmas appropriate for local generated indexes,
- RAM-first staging followed by compact persistence if measurements justify it,
- `VACUUM INTO` or equivalent compact artifact export,
- compressed artifact format such as `tar.zst`,
- source-hash and extractor-version keyed incremental refresh,
- git-diff-aware changed-file detection for CI,
- model download and embedding work outside install,
- separate lexical/graph index readiness from optional embeddings.

Do not borrow:

- broad user-level config mutation,
- hard hooks,
- a sprawling MCP tool surface before Mycelia has a measured need,
- approximate vector indexes without corpus measurements,
- unsafe or custom storage just because it is faster in another project.

## Artifact lifecycle

Commands to design:

```text
mycelia artifact export .mycelia/artifacts/index.tar.zst
mycelia artifact import .mycelia/artifacts/index.tar.zst
mycelia artifact verify .mycelia/artifacts/index.tar.zst
```

Artifact manifest:

```json
{
  "mycelia_version": "",
  "schema_version": 0,
  "project_name": "",
  "git_commit": "",
  "source_root_hash": "",
  "extractors": {},
  "embedding_model": null,
  "created_at": "",
  "db_files": []
}
```

Import must verify:

- project identity,
- schema compatibility,
- extractor compatibility,
- source commit match or acceptable incremental refresh,
- model identity when embeddings are present.

## Index storage modes

V2 keeps SQLite until measurements require otherwise.

Potential refinements:

- one project DB at `.mycelia/db/index.sqlite`,
- optional split embedding store if artifact size or churn demands it,
- optional graph tables populated during code extraction,
- no approximate vector index until brute-force vector search is measured as the
  bottleneck.

## Incremental correctness

Indexer speed cannot weaken correctness.

Still required:

- deterministic identifiers,
- range-addressed UTF-8 chunks,
- extractor version invalidation when boundaries change,
- source hash freshness,
- no stale slice from `retrieve`,
- `find` validation and self-heal for returned top-K,
- eval manifests excluded from discovery,
- invalid UTF-8 and unreadable files counted, not panics.

## CI cache keys

A CI cache key should include:

- OS and architecture only if relevant to artifact portability,
- Mycelia version,
- schema version,
- extractor versions,
- project config hash,
- lockfile or dependency hash only if extractors depend on it,
- git commit or base commit.

Fallback restore should prefer nearest compatible artifact, then refresh.

