# Code-aware chunking

## Plan and ordering

The semantic probe (`08`) closed the lexical-vs-vector question and exposed a
deeper one: the unit being indexed is wrong for source code. The next three
slices fix the indexing unit and the retrieval surface, in this order, because
each depends on the one before it:

1. **`09` code-aware chunking (this doc).** Index syntactic units, not byte
   windows. Everything downstream measures cleaner once chunks are whole symbols.
2. **`10` distilled MCP surface.** Make `find` return ranked headers and
   `retrieve` return bodies, so a client spends tokens only on what it commits to.
3. **`11` precision-first hybrid.** Re-measure embeddings over clean chunks, then
   add a lexical-spine hybrid that expands recall without displacing top lexical
   hits.

Do not reorder. Tuning a hybrid (`11`) over byte-window chunks would tune against
noise; the weights would not survive `09`.

## Why now

`plain-text-v1` chunks on blank lines and a 2,048-byte hard cut. For prose that
is adequate. For code it is structurally wrong, and the evidence shows it:

- A `find` for a paraphrased code intent returns chunks that begin mid-function
  (observed: a `main.rs` hit whose text starts at the tail of one `match` arm).
  The byte range is exact, but the content boundary is arbitrary.
- A chunk that is half of one function and half of the next is not a
  self-contained answer. A client must `retrieve` neighbours or open the file,
  which defeats the token-efficiency goal that justifies the index.
- Embeddings built from arbitrary windows are noisy. Part of the probe's weak
  vector recall on code intent is the boundary, not the model, and that cannot be
  separated until chunks are whole units.

The vision states the thesis as "index symbols and ranges, not files" and names
tree-sitter as the code extractor. The build deferred it through eight slices. The
retrieval plateau on code is the cost of that deferral. This slice closes it.

## Goal

Add a structural extractor that chunks source code on syntactic boundaries, one
chunk per top-level symbol (function, method, `impl`/class, type, top-level
const), each carrying its signature and leading doc comment. Plain-text chunking
remains the extractor for prose and unknown types.

## Locked decisions

- Borrow tree-sitter grammars. Never reimplement a parser. Tree-sitter is a
  parsing dependency, not a retrieval-ranking dependency; this reverses the
  earlier "no tree-sitter for retrieval quality" position only for chunk
  boundaries, not for ranking.
- Select the extractor by content type behind the existing extractor trait.
  Unknown or non-code types keep `plain-text-v1`. No file is rejected for lacking
  a grammar.
- One chunk per top-level symbol. Nested items are addressed in a later slice if
  measurement requires them; do not over-fragment now.
- Each code chunk retains its signature and immediately-preceding doc comment so
  the chunk reads as a unit and so `10` can distill a header from it cheaply.
- Provenance stays exact: tree-sitter node byte ranges map to the existing
  byte-range plus one-based line-range model. No change to the span contract.
- Chunk identifiers stay deterministic and derive from source identity, content
  hash, and range, exactly as today. Re-chunking a changed file re-derives ids.
- Bump the extractor name (for example `tree-sitter-rust-v1`) and record it per
  chunk so a re-chunk invalidates stale chunks and their embeddings.
- Keep the embedding input rule explicit: embed the symbol unit (name +
  signature + doc + body), not a raw byte slice. Do not re-pick the embedding
  model in this slice; re-measure it in `11` over the cleaner chunks.
- Limit the first grammar set to languages present in the Forge corpus (Rust
  first; add only measured languages). Each grammar is a bounded, mechanical add.

## Evaluation

- Preserve the 40-case baseline and the 10-case paraphrase cohort unchanged so
  the before/after is comparable.
- Add a small structural cohort: "find the definition of symbol X" and a few
  code-intent paraphrases whose answer is one function. These measure exactly
  what byte-window chunking cannot serve.
- Record chunk-count change. Symbol chunking should reduce chunk count while
  raising per-chunk relevance; both effects matter.

## Decision gate

Promote structural chunking as the default for code only if it:

1. raises hit rate or MRR on the paraphrase and structural cohorts,
2. loses no more than one established hit on the 40-case baseline,
3. preserves exact provenance and deterministic tie ordering, and
4. keeps index and re-chunk time proportional to changed files.

If a grammar is unavailable or a file fails to parse, fall back to
`plain-text-v1` for that file and count it, never panic.

## Deferred

- Typed edges (`calls`, `imports`, `implements`). This slice produces symbol
  chunks; edges are a later slice once chunks are stable.
- Nested-symbol chunking, cross-file symbol resolution, and call graphs.
- Tree-sitter queries for ranking. Boundaries only, here.
- Any change to the MCP surface shape (that is `10`) or the hybrid (`11`).
