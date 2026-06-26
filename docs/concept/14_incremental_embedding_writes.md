# Incremental embedding writes

## Why now

Slice `13` confirmed that query-class routing improves NL-intent recall. Routing
requires embeddings, and the bakeoff showed that embedding 7,411 chunks takes
547 s on CPU. The original `embed` implementation buffered all generated vectors
in memory and wrote them in a single database transaction at the end. An
interrupted run (Ctrl-C, sleep, OOM) discarded all progress and required
re-embedding from scratch. This is the first structural cost on a fast
index/refresh loop.

## Goal

Make interrupted embed runs resumable with zero user ceremony:

1. Write each batch of embeddings to the database immediately after it is
   computed, before starting the next batch.
2. Because `chunks_missing_embeddings` already filters already-embedded chunks,
   a re-run after interruption only processes the remaining unembedded chunks.
3. Reduce peak memory from O(total corpus vectors) to O(one batch of vectors).

## Locked decisions

- **Batch boundary = commit boundary.** Each batch of `EMBEDDING_BATCH_SIZE`
  chunks is written in a single SQLite transaction. This is faster than
  per-row auto-commit and loses at most one batch on interruption.
- **Cleanup first, embed second.** Stale embeddings from a previous model are
  removed at the start of the refresh, before any batches are processed. This
  separates the cleanup concern from the per-batch write loop and avoids an
  in-transaction delete on every batch.
- **Batch size unchanged at 128.** Per-batch throughput tuning (increasing the
  application-level batch size or the fastembed internal batch size) is a
  separate concern from write granularity. Both affect throughput; only write
  granularity affects resilience.
- **No progress output added.** The `EmbeddingReport` already reports `embedded`
  and `elapsed_ms`. Terminal progress bars or streaming output are UI concerns
  deferred until the embed loop is confirmed fast enough not to need them.

## Changes

`store.rs`:

- Replaced `replace_model_embeddings(database, model_id, dimensions, all_embeddings)`
  with two functions:
  - `remove_other_model_embeddings(database, model_id) -> Result<usize>` — one-shot
    cleanup at the start of a refresh.
  - `upsert_embedding_batch(database, model_id, dimensions, batch) -> Result<()>` —
    per-batch upsert in a transaction.

`embedding.rs`:

- `refresh` calls `remove_other_model_embeddings` once, then for each batch:
  embeds, validates, and calls `upsert_embedding_batch` immediately. Accumulates
  `embedded` count instead of a heap-allocated `Vec` of all vectors.

## Measured result

All 45 unit tests and 7 CLI tests pass. The build is clean. Functional
behaviour is identical: a fresh embed produces the same vectors, a re-run
produces `embedded: 0`, and changing models removes the old embeddings and
re-embeds under the new model ID.

The resilience property is structural and was not measured under fault injection.

## Deferred

- Batch size tuning (128 → 256 or 512 to improve CPU throughput per fastembed
  call, trading memory for speed).
- A faster local model. `BAAI/bge-small-en-v1.5` is 33 M params; if the refresh
  ceiling remains a problem after typed-edge extraction increases corpus size,
  evaluate a smaller or hardware-accelerated model.
- Terminal progress output (percentage complete, estimated time remaining).
- Parallel batch embedding across model replicas or ONNX execution providers.
