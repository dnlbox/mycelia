# Mycelia v1 — build state

Working memory for the looping build agent. **Read first, update last, every slice.** The team lead reviews at each go/no-go gate; the build agent never crosses a gate on its own. See [prompt.md](prompt.md) for the loop, [ROADMAP.md](ROADMAP.md) for the phases.

## Position

- **Phase:** 1 — Per-commit index + CI artifact
- **Slice:** 1 complete — `mycelia ci prepare`
- **Status:** Phase 1 in progress. `ci prepare` implemented; next item is artifact export/import/verify.
- **Tree:** green (2026-06-29: fmt, clippy, tests, release build, install, CLI smoke, paired eval run, MCP smoke, stats)

## Next up

Phase 1, second item: implement artifact `export` / `import` / `verify` with the manifest format (R7). See [ROADMAP.md](ROADMAP.md) Phase 1 and requirements R7 / R8.

## Gate status

- [x] **GO/NO-GO 0** — determinism + measurement baseline (**GREEN — lead-reviewed 2026-06-29**)
- [ ] GO/NO-GO 1 — per-commit index + CI artifact
- [ ] GO/NO-GO 2 — change-scoped retrieval
- [ ] GO/NO-GO 3 — Vercel AI SDK 7.0 integration
- [ ] GO/NO-GO 4 — SHIP

## GO/NO-GO 0 evidence

- [x] Index built twice at the same SHA is byte-identical (R3): release smoke indexed the same fixture corpus into two independent databases; ordered chunk-id sets matched exactly (`chunk_ids_identical=true`, `deterministic_chunk_ids=2`). Regression test: `store::tests::indexing_same_tree_twice_produces_identical_chunk_ids`.
- [x] Eval harness emits all three metrics on paired runs, reproducibly: `mycelia eval --paired --json` on `fixtures/eval/mycelia-v1-code.json` reported Mycelia 5/5 hits, baseline 4/5 hits, Mycelia tokens/answer 1219.8, baseline tokens/answer 29445.25, with MRR and comparison deltas present.
- [x] `find` works with no embeddings and no model cache (R6 proxy until Phase 1 `ci prepare` exists): release smoke ran `setup --no-embed`, then default `find --corpus lexical --json`; it returned one hit and `model_cache_created=false`. Regression test: `setup_no_embed_supports_default_find_without_model_cache`.

## Done log (append-only, terse — newest last)

- 2026-06-29 — v1 reset: docs + roadmap + build-loop established; engine inherited from `main` (tree-sitter chunking for Rust/TS/TSX/Python/Ruby, SQLite schema v5, deterministic chunk IDs, freshness, read-only MCP, Rust calls graph). Nothing built against the v1 roadmap yet.
- 2026-06-29 — Phase 0 / Slice 1: added v1 fixed-task eval schema (`required_files`), emitted required files in eval results, rejected in-corpus eval manifests at runtime, added `fixtures/eval/mycelia-v1-code.json` with five code-only tasks, and documented the manifest contract. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; CLI smoke ok; eval run 5/5 hits, MRR 0.8000, tokens/answer 1219.8; MCP smoke ok; stats ok.
- 2026-06-29 — Phase 0 / Slice 2: added `mycelia eval --paired` with a deterministic live-file `grep_read` baseline, paired JSON/text output (`mycelia`, `baseline`, `comparison`), comparison deltas, and tests. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; CLI smoke ok; paired eval run on `fixtures/eval/mycelia-v1-code.json` reported Mycelia 5/5 hits, baseline 4/5 hits, Mycelia tokens/answer 1219.8, baseline tokens/answer 29128.75; MCP smoke ok; stats ok.
- 2026-06-29 — Phase 0 / Slice 3: added full-index deterministic chunk-id regression coverage and explicit `setup --no-embed` default-find regression coverage. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; release smoke ok: chunk-id sets identical across two independent indexes, lexical default find returned 1 hit with no model cache, paired eval emitted all gate metrics, MCP smoke ok, stats ok. Stopped at GO/NO-GO 0 for lead review.
- 2026-06-29 — **LEAD REVIEW → GO/NO-GO 0 = GREEN.** Independently verified: full suite green (89 core + 54 CLI tests, 0 fail); determinism test passes; scope clean (no Phase-1 leakage); R11 in-corpus-manifest guard confirmed enforced at runtime. Reproduced `eval --paired` twice (byte-identical): Mycelia 5/5 hits, MRR 0.8, 1219.8 tok/ans vs baseline 4/5, 29445.2 tok/ans; token-reduction ratio 0.9586. Cleared to start Phase 1.
- 2026-06-29 — Phase 1 / Slice 1: implemented `mycelia ci prepare` with project-local `.mycelia/db/index.sqlite3`, real `HEAD` binding, schema validation, R8 cache-key composition (`mycelia_version`, schema version, extractor-version hash, project-config hash, git commit), GitHub env emission, lexical-only CI by default (`--embed` opt-in), JSON/text reports, and regression tests for lexical indexing, env output, stable cache keys, and config-hash invalidation. Also excluded `.mycelia/` internal state from corpus discovery after smoke caught generated files being indexed. Validation: fmt ok; clippy ok; workspace tests ok (89 core + 28 CLI-unit + 28 CLI integration, 0 fail); release build ok; install ok; isolated default `ci prepare` smoke indexed 1 source file and returned a header-only `find` hit; paired eval smoke hit rate 1.0 / MRR 1.0 / 118.0 tokens per answer; MCP stdio smoke initialized, listed 6 tools, called `find`, and exited cleanly; stats recorded 1 tiny-corpus query.

## Decisions

- Phase 0 eval tasks use `required_files` as the file-level oracle. Legacy `expected` manifests remain supported for diagnostic fixtures, but v1 gate evidence should use `required_files`.
- `mycelia eval` rejects a manifest inside the indexed corpus. Fixture manifests under `fixtures/eval/` are still excluded from discovery, and measurement runs should copy/reference the manifest from outside the corpus under test.
- Paired Phase 0 reporting uses `mycelia eval --paired`. The baseline is named `grep_read`: it ranks live files under the same corpus root with deterministic lexical scoring and bills bytes read through the first required file.
- Phase 0 R6 evidence is a proxy over the current surface (`setup --no-embed` plus default `find`) because `mycelia ci prepare --no-embed` is a Phase 1 roadmap item and must not be implemented before GO/NO-GO 0 is reviewed.
- (lead, 2026-06-29) The Phase 0 eval set is intentionally small (5 tasks) and keyword-shaped — adequate to prove the harness emits reproducible metrics, which is all GO/NO-GO 0 asks. It is NOT a representative benchmark: the 95.86% reduction must not be cited as the ship proof. Phase 4's benchmark must use realistic natural-language queries and a larger labelled task set (see [docs/evaluation.md](docs/evaluation.md)). Carry `ci prepare --no-embed` forward in Phase 1 to convert the R6 proxy into direct evidence.
- `ci prepare` requires a real Git `HEAD`; fake `.git` directories remain acceptable for legacy local setup tests, but CI cache evidence must bind to an actual commit.
- `.mycelia/` is internal state and is excluded from discovery. Generated guidance, config, logs, caches, artifacts, and SQLite files must not contaminate the indexed corpus.
- Phase 1 Slice 1 converts the R6 proxy into direct evidence for default `ci prepare` lexical mode plus `--no-embed` / `--lexical`; broader R8 gate evidence still waits on artifact import/export and incremental refresh.

## Blockers / questions for the lead

- (none)
