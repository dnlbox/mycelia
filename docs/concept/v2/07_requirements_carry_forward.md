# Requirements carry-forward

V2 is a rewrite of the adoption model, not permission to lose hard-won
requirements. This document tracks the requirements that must survive.

## Retrieval contract

Keep:

- `find` returns distilled headers under a token budget.
- `retrieve` returns full selected bodies.
- Tokens per answered query remains a first-class gate.
- Hit rate and MRR are not enough if token cost regresses.
- Header results include path, range, score, chunk id, and signature or synopsis.

V2 implication:

- CI seed context should be built from `find` headers, not raw full-file reads.

## Freshness contract

Keep:

- Never serve a stale indexed slice.
- `retrieve` returns precise chunk only when the source hash still matches.
- If the source changed, return the whole current live file.
- If the source vanished, escaped the root, became unreadable, or is no longer
  text, return `unavailable`.
- `find` validates returned top-K sources, self-heals drift, and re-ranks once.
- Server self-heal is internal maintenance, not a model-facing mutation tool.

V2 implication:

- Project-local DBs and CI-restored artifacts must still validate against the
  checkout before answering.

## Project and corpus resolution

Keep:

- A model should not pass arbitrary database paths.
- Explicit path/database mode remains for fixtures and diagnostics.
- Current project scope is inferred from cwd.
- Cross-project search is explicit, never silent search-all.
- Chunk ids carry corpus or project identity when needed.

V2 changes:

- Prefer `.mycelia/config.toml` over user-level named corpus profiles.
- Preserve legacy profiles as migration/fallback, not the center of the journey.

## MCP surface

Keep:

- One read-oriented MCP server.
- `find`, `retrieve`, `search_codebase`, `locate_implementation`,
  `list_corpora` or project equivalent, and `find_related`.
- stdout reserved for protocol.
- diagnostics on stderr.
- no model-facing mutation tools by default.

V2 changes:

- One MCP discovers project context from `.mycelia/`.
- `index_repository` as an MCP mutation tool remains out of scope unless consent
  and CI use cases are designed separately.

## Graph requirements

Keep:

- Deterministic, sourced typed edges.
- Conservative resolution: a wrong connection is worse than none.
- Ambiguous names return every candidate as unresolved rather than silently
  picking one.
- Query-time resolution for edges where incremental reindex could otherwise make
  stored target ids stale.
- `find_related` remains one parameterized tool to avoid tool-surface sprawl.

Pending:

- TypeScript, Python, and Ruby edges.
- Additional edge types such as imports and implements.
- Method-call resolution only with type information.
- Traversal beyond depth 1 after correctness is measured.

## Indexing and extraction

Keep:

- Content-agnostic core.
- Code-aware tree-sitter chunking where grammar support exists.
- Plain-text fallback for unknown or failed parse cases.
- Borrow parsers, do not reimplement grammars.
- Bump extractor identifier when chunk boundaries change.
- Deterministic discovery and result ordering.
- Ignore eval manifests so oracle queries and answers never contaminate corpus
  retrieval.

V2 additions:

- Parallelize only where deterministic final ordering is preserved.
- Add git-aware incremental refresh for CI.
- Add artifact import/export verification before specialized storage.

## Embeddings and ranking

Keep:

- Embedding providers behind a narrow core trait.
- Record model identifier and vector dimensions.
- Setup/embed are the only paths that may download embedding models.
- Install, find, serve, connect, and init must not trigger hidden model
  downloads.
- Lexical and vector strategies stay independently selectable while measured.
- Brute-force vector similarity remains acceptable until measurements prove the
  need for an approximate index.

V2 implication:

- CI should be able to run lexical/graph-only if model download is not allowed.

## Observability

Keep:

- Activity logs are best-effort and must not interrupt tool handling.
- `stats` reads logs and reports query count, actual tokens, cold-read estimate,
  and savings ratio.
- `status` reports DB health and names the fix.
- Dogfood evidence is required at slice closeout.

V2 changes:

- `stats` and `status` default to the current project.
- `stats --all` spans project-local and legacy registered indexes.
- CI emits machine-readable summaries.

## Distribution

Keep:

- Install provides the binary, not hidden corpus setup.
- Model download belongs to setup/embed, not install.
- Homebrew builds avoid ORT binary downloads and use system ONNX Runtime.
- The gold path remains simple, but v2 changes it:

```text
brew install mycelia
cd project
mycelia init
mycelia serve
```

## Consent and project writes

Keep and strengthen:

- No hidden mutation of project instruction files.
- No user-level config writes by default.
- Any write outside `.mycelia/` requires preview and confirmation.
- Generated Mycelia-owned blocks must be idempotent and removable.

## Deferred from v1 that still matters

- `stats --all`.
- Hide or remove retired `corpus` command from visible CLI help.
- Debounced watcher as latency optimization.
- Retrieval-quality work on remaining Forge eval misses under token gate.
- Graph extension beyond Rust calls.
- Faster embedding/index throughput.
- Artifact/cache strategy for CI.
- Federation only after local project workflow is solid.

