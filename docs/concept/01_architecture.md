# mycelia — architecture

## Core model: everything is a chunk

A **chunk** is a range-addressable span of any source (a function, a paragraph, a
slide, a spreadsheet region, a transcript window). Each chunk carries:

- **embedding(s)** — from a swappable backend, re-embeddable as models improve;
- **typed edges** — `calls`, `imports`, `implements`, `derived_from`,
  `semantically_similar`, `supersedes`, `example_of`, … ;
- **provenance** — exact source span + extractor + a confidence tag
  (EXTRACTED / INFERRED / AMBIGUOUS + score).

Code is a first-class extractor, not the centre. The same core indexes a repo and a
case file.

## Pipeline

`discover → extract → embed → store → query → serve (MCP)`

- **discover** — walk + ignore (existential: `.gitignore` + a forge ignore; the
  corpus is ~97% generated JSON). Per-path sensitivity; never `~/Code/`. FSEvents
  watch for live changes.
- **extract** — pluggable per content type: tree-sitter for code (symbols, calls,
  imports — borrowed, not reimplemented); chunkers for prose / PDF / office /
  transcripts. Sub-file granularity throughout.
- **embed** — swappable backend (candle / ORT / llama.cpp locally, Metal-
  accelerated; or an API). The model id is recorded per embedding; a model change
  triggers a targeted re-embed.
- **store** — owned, cache-friendly: packed struct-of-arrays, string/symbol
  interning, arena allocation, `mmap` + zero-copy (rkyv), columnar layout. Metadata
  in SQLite or a custom store; vectors in a quantised index.
- **query** — fan-out across corpora (federation); rank; return distilled slices.
- **serve** — MCP, the one interface every client shares.

## The performance core (where the mainframe instincts compound)

These are candidate optimizations, not first-milestone commitments. Representative
corpora must demonstrate that the baseline storage or retrieval path misses a
measured target before any candidate is adopted:

- **Vectors** — HNSW or IVF-PQ; product/scalar quantisation (bit-pack so millions of
  vectors fit in RAM); SIMD distance kernels (NEON / Metal on Apple Silicon).
- **Freshness** — BLAKE3 / xxHash content hashes; **roaring bitmaps** for the
  affected-set; incremental partial reindex; never a full rescan.
- **Layout** — interned tables, packed bitfields, struct-of-arrays —
  cache-friendliness is the game at scale.

The first milestone uses SQLite metadata and lexical retrieval behind narrow
interfaces. This is not the final retrieval architecture. It is a reference path
for measuring chunk counts, index time, update cost, query behavior, and provenance
correctness before selecting embeddings, vector indexes, or packed layouts.

## Federation (the cross-corpus case)

Multiple corpora, queried together. A query fans out across indexes and can form
cross-corpus edges (a case × a body of jurisprudence). Start single-corpus; design
the query layer to fan out from day one.

## MCP surface

`index`, `retrieve`, `find`, `ignore`, plus typed queries (premises-for-X,
decisions-in-Y, examples-of-Z, cross-corpus-find). Returns ranked, sourced,
distilled slices — never raw files. That is the token win.

Status: the implemented read-only surface (`find`, `retrieve`) is two-stage:
`find` returns ranked sourced headers without chunk bodies, and `retrieve` returns
the chosen body. The shape is specified in `10_distilled_mcp_surface.md`.

## Security

The index is a distilled map of everything I have built — a sensitive asset.
Local-only; encrypted-at-rest option; capability-scoped MCP; per-path sensitivity;
memory-safe (Rust). It never reaches `~/Code/`.

## Tone-down (aim for the moon, land safely)

Build for my corpus + content types first; federation can start single. Typed-edge
richness starts shallow (lean on the protocol files — concept / AGENTS /
BUILD_STATE — and tree-sitter; enrich with cheap-model passes). The bit-level work
goes only where it compounds (vectors, freshness, layout), not into parsers or the
embedding model.

The implementation sequence is:

1. Prove deterministic `discover → extract → store → find` over UTF-8 text. (done)
2. Measure representative personal and large-corpus fixtures. (done: lexical
   baseline and a semantic probe; see `03`–`08`.)
3. Fix the indexing unit before further ranking work: code-aware (tree-sitter)
   chunking on syntactic boundaries (`09`), so chunks are whole symbols.
4. Make the surface distilled and two-stage so retrieval saves tokens against
   reading files (`10`). (done)
5. Re-measure embeddings over clean chunks, then add a precision-first hybrid
   that expands recall without displacing top lexical hits (`11`).
6. Add typed edges only where they improve evaluated queries.
7. Add federation and specialized storage when corpus measurements require them.

The evidence reordered this sequence. Structural extraction (step 3) was
originally lumped with hybrid retrieval; the semantic probe showed that hybrid
tuning over byte-window chunks tunes against noise, so the indexing unit must be
fixed first.
