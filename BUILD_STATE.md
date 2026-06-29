# Mycelia v1 — build state

Working memory for the looping build agent. **Read first, update last, every slice.** The team lead reviews at each go/no-go gate; the build agent never crosses a gate on its own. See [prompt.md](prompt.md) for the loop, [ROADMAP.md](ROADMAP.md) for the phases.

## Position

- **Phase:** 0 — Measurement baseline & determinism proof
- **Slice:** Phase 0 / Slice 3 complete
- **Status:** GO/NO-GO 0 awaiting lead review
- **Tree:** green (2026-06-29: fmt, clippy, tests, release build, CLI smoke, paired eval run, MCP smoke, stats)

## Next up

GO/NO-GO 0 lead review. Do not start Phase 1 until the lead marks the gate GREEN.

## Gate status

- [ ] **GO/NO-GO 0** — determinism + measurement baseline (**AWAITING LEAD REVIEW**)
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

## Decisions

- Phase 0 eval tasks use `required_files` as the file-level oracle. Legacy `expected` manifests remain supported for diagnostic fixtures, but v1 gate evidence should use `required_files`.
- `mycelia eval` rejects a manifest inside the indexed corpus. Fixture manifests under `fixtures/eval/` are still excluded from discovery, and measurement runs should copy/reference the manifest from outside the corpus under test.
- Paired Phase 0 reporting uses `mycelia eval --paired`. The baseline is named `grep_read`: it ranks live files under the same corpus root with deterministic lexical scoring and bills bytes read through the first required file.
- Phase 0 R6 evidence is a proxy over the current surface (`setup --no-embed` plus default `find`) because `mycelia ci prepare --no-embed` is a Phase 1 roadmap item and must not be implemented before GO/NO-GO 0 is reviewed.

## Blockers / questions for the lead

- (none)
