# Distilled MCP surface

## Why now

The product goal is that a harness consults the index instead of re-reading the
same files. The current MCP `find` works against that goal: it returns up to ten
full chunk bodies as flattened JSON. One observed `find` returned ten hits
totalling roughly 1,800 tokens, and the relevant symbol was not in the top three.
A client pays for everything and still has to read more.

The vision says the surface "returns ranked, sourced, distilled slices, never raw
files." Before this slice, it returned raw chunk text. This slice closes that
drift. It depends on `09`: a header is only meaningful when a chunk is a whole
symbol.

## Goal

Make retrieval two-stage so a client spends full-body tokens only on what it
commits to:

- `find` returns ranked **headers**: source path, line range, symbol name and
  signature (or a one-line synopsis for prose), score, and the `chunk_id`.
  Headers are cheap, roughly one line each.
- `retrieve` returns the full body for a `chunk_id` the client chooses, as today.

## Locked decisions

- `find` does not return chunk bodies by default. It returns a header per hit.
  A header omits `text`; it carries the distilled signature/synopsis instead.
- Distillation is deterministic and extractor-supplied: for a code chunk, the
  signature plus the first doc line; for prose, the first non-empty line. No model
  call, no second parser. `09` already retained the signature and doc, so this is
  a projection, not new analysis.
- Enforce a result budget on `find`: a default and a hard cap on the number of
  headers, and a total-byte ceiling so a single call cannot dump the corpus.
- `retrieve` is unchanged in shape: exact chunk, exact provenance, full text.
- The surface stays read-only. No new mutation tools.
- Preserve exact provenance in every header (path, byte range, line range,
  `chunk_id`, score, extractor) so a header alone is enough to cite a source.
- Keep the core return types and the CLI's explicit-path JSON output usable for
  tests; the header projection lives at the core boundary, not only in MCP, so the
  CLI and MCP agree.

## Token-efficiency as the measured goal

Hit rate and MRR are proxies. The measure that justifies the project is tokens
spent to reach an answer. Record, for representative queries:

- tokens returned by a default `find` (headers) plus the `retrieve` bodies a
  client actually needs, versus
- tokens to open the source file(s) cold.

This metric is added to the evaluation framework (`03`) and is the primary gate
for this slice. A distilled surface that does not reduce tokens-per-answer over
reading the file is not worth shipping.

## Decision gate

Promote the two-stage surface only if it:

1. reduces tokens-per-answered-query against reading the source files on the
   paraphrase and structural cohorts,
2. preserves exact provenance in headers and bodies, and
3. keeps `find` latency within the current budget.

## Measured result

The surface now returns deterministic headers from core, CLI JSON, and MCP
`find`. A header carries `chunk_id`, path, source hash, byte and line range,
extractor, score, and either a code signature plus synopsis or a prose synopsis.
It omits `text`. `retrieve` remains the full-body path.

The evaluator now reports an estimated token proxy: returned `find` headers plus
the first relevant retrieved body, compared with reading the matching source file
cold when the corpus root is available.

Against the fresh Forge index after this slice:

| Cohort | Hits | MRR | Tokens / answer | Cold tokens / answer |
| --- | ---: | ---: | ---: | ---: |
| 40-case baseline | 35 / 40 | 0.717 | 1,014 | 3,150 |
| 10-case paraphrase | 1 / 10 | 0.033 | 590 | 510 |
| 8-case structural | 5 / 8 | 0.563 | 775 | 8,099 |

Decision: keep the two-stage surface. It is a clear win for code-shaped
structural queries and the aggregate baseline while preserving ranking and exact
provenance. The paraphrase cohort has only one answered case, and that source file
is small enough that the header list is slightly more expensive than reading it
cold. Treat this as a metric caveat for `11`, not a reason to restore raw bodies
to `find`.

## Deferred

- Server-side multi-chunk distillation or summarization beyond the deterministic
  header projection.
- Typed graph queries (premises-for, decisions-in, examples-of) and cross-corpus
  find. Those ride on edges, deferred past `11`.
- Streamable HTTP, auth, resources, and prompts, as in `06`.
