# Embedding throughput

## Why now

Slice `15` added TypeScript and Python structural extraction, growing the Forge
corpus from 7,411 to 9,410 chunks. The initial full-corpus embed (before slice
`14`'s incremental writes) took 547 s for 7,411 chunks on CPU. Projecting to
9,410 chunks gives roughly 695 s (~12 min). That is too slow for a comfortable
`embed` run, and the process provides no progress feedback — it looks frozen.

Two orthogonal concerns:

1. **Throughput**: how long does a full embed actually take?
2. **Observability**: can the user tell it is still running?

Slice `14`'s incremental writes address resilience (an interrupted run resumes
from where it left off). This slice addresses throughput and observability.

## Important: incremental nature of re-embeds

After a re-index that only adds new chunks (e.g., adding TS/Py extraction to an
already-embedded corpus), `mycelia embed` only processes chunks that do not yet
have embeddings. In the slice-15 case: ~7,411 chunks already had embeddings;
only ~1,999 new structural chunks needed embedding. That run would take ~2–3 min,
not 12 min. The 12-min figure applies to a cold start from zero.

## Changes

**`semantic.rs`** — use all available CPU cores for ONNX parallelism:

```rust
let cpu_count = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
.with_intra_threads(cpu_count)
```

Previously hard-coded `with_intra_threads(4)`. On an 8-core Apple M-series
machine, this doubles the BLAS threads available for matrix multiply, which is
the innermost hot loop of the ONNX inference.

**`embedding.rs`** — double the application-level batch size:

```rust
const EMBEDDING_BATCH_SIZE: usize = 256; // was 128
```

Halves the number of ONNX inference calls for the same corpus. The fastembed
default internal batch is 256; matching it at the application level means each
`embed_documents` call fills one full ONNX batch.

**`embedding.rs`** — live progress counter on stderr:

```
embedding 0/9410...
embedding 256/9410...
embedding 512/9410...
...
```

Each batch overwrites the previous line via `\r`. The final newline is printed
when done. No output when there is nothing to embed (unchanged run). Stderr is
used so JSON stdout (`--json`) is unaffected.

## Expected impact

- **Batch count**: 9,410 chunks / 256 per batch = 37 batches (was 74)
- **Thread count**: 8 instead of 4 on an 8-core machine
- Combined: potentially 2–4× throughput improvement on Apple Silicon, bringing
  a cold-start full embed from ~12 min down to 3–6 min
- Incremental embeds (the common case after a re-index) remain short: only
  newly indexed chunks need embedding

These numbers are estimates pending a measured run. The actual speedup depends on
BLAS/Accelerate utilization at larger batch sizes and whether `intra_threads`
above 4 is respected by the ONNX runtime on Apple Silicon.

## Deferred

- **Apple Neural Engine / CoreML provider**: ONNX on CPU is the bottleneck.
  Running through Apple's Neural Engine (via the CoreML execution provider)
  could be 10–50× faster but requires a fastembed build with CoreML support,
  which is not available in the current prebuilt ORT binaries.
- **Quantized models**: a quantized version of `bge-small-en-v1.5` (INT8) would
  be 2–4× faster at roughly equal retrieval quality. Requires evaluating model
  parity.
- **Estimated time remaining**: add elapsed time and ETA to the progress line
  once a measured baseline is established.
