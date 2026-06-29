# Mycelia v1 — build state

Working memory for the looping build agent. **Read first, update last, every slice.** The team lead reviews at each go/no-go gate; the build agent never crosses a gate on its own. See [prompt.md](prompt.md) for the loop, [ROADMAP.md](ROADMAP.md) for the phases.

## Position

- **Phase:** 1 — Per-commit index + CI artifact
- **Slice:** 3 complete — git-diff-aware incremental refresh
- **Status:** GO/NO-GO 1 AWAITING LEAD REVIEW. Build agent must stop here; do not start Phase 2 until lead review marks the gate green.
- **Tree:** green (2026-06-29: fmt, clippy, tests, release build, install, CLI smoke, paired eval run, MCP smoke, stats)

## Next up

GO/NO-GO 1 lead review. Review the evidence below against R7 / R8 / R10 before deciding whether Phase 2 may start.

## Gate status

- [x] **GO/NO-GO 0** — determinism + measurement baseline (**GREEN — lead-reviewed 2026-06-29**)
- [ ] **GO/NO-GO 1** — per-commit index + CI artifact (**AWAITING LEAD REVIEW**)
- [ ] GO/NO-GO 2 — change-scoped retrieval
- [ ] GO/NO-GO 3 — Vercel AI SDK 7.0 integration
- [ ] GO/NO-GO 4 — SHIP

## GO/NO-GO 0 evidence

- [x] Index built twice at the same SHA is byte-identical (R3): release smoke indexed the same fixture corpus into two independent databases; ordered chunk-id sets matched exactly (`chunk_ids_identical=true`, `deterministic_chunk_ids=2`). Regression test: `store::tests::indexing_same_tree_twice_produces_identical_chunk_ids`.
- [x] Eval harness emits all three metrics on paired runs, reproducibly: `mycelia eval --paired --json` on `fixtures/eval/mycelia-v1-code.json` reported Mycelia 5/5 hits, baseline 4/5 hits, Mycelia tokens/answer 1219.8, baseline tokens/answer 29445.25, with MRR and comparison deltas present.
- [x] `find` works with no embeddings and no model cache (R6 proxy until Phase 1 `ci prepare` exists): release smoke ran `setup --no-embed`, then default `find --corpus lexical --json`; it returned one hit and `model_cache_created=false`. Regression test: `setup_no_embed_supports_default_find_without_model_cache`.

## GO/NO-GO 1 evidence

- [x] `export` → `import` → `verify` round-trips; mismatched manifest field is rejected with a named reason (R7): `ci_artifact_export_verify_import_round_trips_project_index` deletes the project-local DB, imports the artifact, then finds the restored symbol. `ci_artifact_verify_rejects_named_manifest_mismatch` tampers `git_commit` and receives `artifact mismatch: git_commit`.
- [x] Cache key changes iff a composing input changes; unchanged tree reuses artifact byte-for-byte (R8): `ci_prepare_cache_key_is_stable_until_project_config_changes` confirms repeated prepare on the same commit/config keeps the cache key stable and changing `.mycelia/config.toml` changes it. Slice 1 also covers schema/extractor/version/git inputs in the emitted key. Artifact byte-for-byte reuse is indirectly covered for unchanged restored DB files; a dedicated artifact-byte comparison on a larger fixture is still useful for lead review.
- [x] Cold build within CI setup budget; warm incremental refresh in seconds (R10): generated 120-file fixture reported cold `ci prepare` 0.032s and warm `ci prepare --restore` 0.035s with `changed_paths=3`, `files_indexed=2`, `files_removed=1`. This is a small fixture, not a representative medium repo benchmark.

## Done log (append-only, terse — newest last)

- 2026-06-29 — v1 reset: docs + roadmap + build-loop established; engine inherited from `main` (tree-sitter chunking for Rust/TS/TSX/Python/Ruby, SQLite schema v5, deterministic chunk IDs, freshness, read-only MCP, Rust calls graph). Nothing built against the v1 roadmap yet.
- 2026-06-29 — Phase 0 / Slice 1: added v1 fixed-task eval schema (`required_files`), emitted required files in eval results, rejected in-corpus eval manifests at runtime, added `fixtures/eval/mycelia-v1-code.json` with five code-only tasks, and documented the manifest contract. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; CLI smoke ok; eval run 5/5 hits, MRR 0.8000, tokens/answer 1219.8; MCP smoke ok; stats ok.
- 2026-06-29 — Phase 0 / Slice 2: added `mycelia eval --paired` with a deterministic live-file `grep_read` baseline, paired JSON/text output (`mycelia`, `baseline`, `comparison`), comparison deltas, and tests. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; CLI smoke ok; paired eval run on `fixtures/eval/mycelia-v1-code.json` reported Mycelia 5/5 hits, baseline 4/5 hits, Mycelia tokens/answer 1219.8, baseline tokens/answer 29128.75; MCP smoke ok; stats ok.
- 2026-06-29 — Phase 0 / Slice 3: added full-index deterministic chunk-id regression coverage and explicit `setup --no-embed` default-find regression coverage. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; release smoke ok: chunk-id sets identical across two independent indexes, lexical default find returned 1 hit with no model cache, paired eval emitted all gate metrics, MCP smoke ok, stats ok. Stopped at GO/NO-GO 0 for lead review.
- 2026-06-29 — **LEAD REVIEW → GO/NO-GO 0 = GREEN.** Independently verified: full suite green (89 core + 54 CLI tests, 0 fail); determinism test passes; scope clean (no Phase-1 leakage); R11 in-corpus-manifest guard confirmed enforced at runtime. Reproduced `eval --paired` twice (byte-identical): Mycelia 5/5 hits, MRR 0.8, 1219.8 tok/ans vs baseline 4/5, 29445.2 tok/ans; token-reduction ratio 0.9586. Cleared to start Phase 1.
- 2026-06-29 — Phase 1 / Slice 1: implemented `mycelia ci prepare` with project-local `.mycelia/db/index.sqlite3`, real `HEAD` binding, schema validation, R8 cache-key composition (`mycelia_version`, schema version, extractor-version hash, project-config hash, git commit), GitHub env emission, lexical-only CI by default (`--embed` opt-in), JSON/text reports, and regression tests for lexical indexing, env output, stable cache keys, and config-hash invalidation. Also excluded `.mycelia/` internal state from corpus discovery after smoke caught generated files being indexed. Validation: fmt ok; clippy ok; workspace tests ok (89 core + 28 CLI-unit + 28 CLI integration, 0 fail); release build ok; install ok; isolated default `ci prepare` smoke indexed 1 source file and returned a header-only `find` hit; paired eval smoke hit rate 1.0 / MRR 1.0 / 118.0 tokens per answer; MCP stdio smoke initialized, listed 6 tools, called `find`, and exited cleanly; stats recorded 1 tiny-corpus query.
- 2026-06-29 — Phase 1 / Slice 2: implemented same-checkout CI artifact `export` / `verify` / `import` with `manifest.json`, copied SQLite db files, required R7 fields (`mycelia_version`, `schema_version`, `project_name`, `git_commit`, `source_root_hash`, `extractors`, `embedding_model`, `db_files`), named mismatch errors, source-root hashing through the same discovery rules as indexing, and a corpus-root guard so imports cannot install an artifact whose stored root would break freshness. Validation: fmt ok; clippy ok; workspace tests ok (90 core + 28 CLI-unit + 30 CLI integration, 0 fail); release build ok; install ok; isolated smoke prepared, exported, verified, deleted db, imported, and found the restored symbol; paired eval smoke hit rate 1.0 / MRR 1.0 / 120.0 tokens per answer; MCP stdio smoke initialized, listed 6 tools, called `find`, and exited cleanly; stats recorded 1 tiny-corpus query.
- 2026-06-29 — Phase 1 / Slice 3: implemented `ci prepare --restore <artifact>` for previous-commit artifact restore followed by git-diff-aware changed-path refresh; added core `refresh_changed_sources` that updates a supplied relative path set without pruning untouched indexed sources; filtered internal `.mycelia` / VCS paths from git diff; kept exact `ci import` strict while restore mode skips only expected previous-commit `git_commit` and `source_root_hash` mismatches. Validation: fmt ok; clippy ok; workspace tests ok (91 core + 28 CLI-unit + 31 CLI integration, 0 fail); release build ok; install ok; isolated restore smoke cold-prepared/exported commit A, changed/deleted/added files at commit B, restored artifact, refreshed `changed_paths=3`, indexed 2 files, removed 1 file, preserved stable file, and dropped deleted file; paired eval smoke hit rate 1.0 / MRR 1.0 / 120.0 tokens per answer; MCP stdio smoke initialized, listed 6 tools, called `find`, and exited cleanly; stats recorded 1 tiny-corpus query. Stopped at GO/NO-GO 1 for lead review.

## Decisions

- Phase 0 eval tasks use `required_files` as the file-level oracle. Legacy `expected` manifests remain supported for diagnostic fixtures, but v1 gate evidence should use `required_files`.
- `mycelia eval` rejects a manifest inside the indexed corpus. Fixture manifests under `fixtures/eval/` are still excluded from discovery, and measurement runs should copy/reference the manifest from outside the corpus under test.
- Paired Phase 0 reporting uses `mycelia eval --paired`. The baseline is named `grep_read`: it ranks live files under the same corpus root with deterministic lexical scoring and bills bytes read through the first required file.
- Phase 0 R6 evidence is a proxy over the current surface (`setup --no-embed` plus default `find`) because `mycelia ci prepare --no-embed` is a Phase 1 roadmap item and must not be implemented before GO/NO-GO 0 is reviewed.
- (lead, 2026-06-29) The Phase 0 eval set is intentionally small (5 tasks) and keyword-shaped — adequate to prove the harness emits reproducible metrics, which is all GO/NO-GO 0 asks. It is NOT a representative benchmark: the 95.86% reduction must not be cited as the ship proof. Phase 4's benchmark must use realistic natural-language queries and a larger labelled task set (see [docs/evaluation.md](docs/evaluation.md)). Carry `ci prepare --no-embed` forward in Phase 1 to convert the R6 proxy into direct evidence.
- `ci prepare` requires a real Git `HEAD`; fake `.git` directories remain acceptable for legacy local setup tests, but CI cache evidence must bind to an actual commit.
- `.mycelia/` is internal state and is excluded from discovery. Generated guidance, config, logs, caches, artifacts, and SQLite files must not contaminate the indexed corpus.
- Phase 1 Slice 1 converts the R6 proxy into direct evidence for default `ci prepare` lexical mode plus `--no-embed` / `--lexical`; broader R8 gate evidence still waits on artifact import/export and incremental refresh.
- Artifact import is intentionally same-checkout for now: `verify` rejects a database whose stored `corpus_root` differs from the current checkout root. Cross-path artifact rebinding is not part of this slice and would need explicit core support because freshness and retrieve use the stored root.
- `ci prepare --restore` is the sanctioned previous-commit path. It verifies version/schema/project/extractor/db/embedding/corpus-root compatibility, skips only previous-commit `git_commit` and `source_root_hash`, installs the artifact, computes `git diff old..HEAD`, filters internal paths, and refreshes just those changed paths.

## Blockers / questions for the lead

- (none)
