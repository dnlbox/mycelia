# Definition ranking and routed default

## Why now

With the full corpus embedded and current (11,969 chunks), the routed strategy
overtook the standing default on the 68-case Forge gate
(baseline 40 + expanded 18 + paraphrase 10):

| strategy | hits | tokens/answer |
| --- | --- | --- |
| fts5-reranked (prior default) | 47/68 | 1289.5 |
| routed | 50/68 | 1295.7 |

Routed won by +3 at parity token cost (+0.5%). Slice `13` had deferred promoting
routed "until typed-edge work shows whether remaining misses are routing failures
or structural missing context." A miss analysis answered that question without
building typed edges: of routed's 18 misses, all but one were paraphrase/intent
recall failures, and the lone exact-symbol miss (`candelabrum exact RunStore`)
was a ranking failure, not a missing-context failure. `store.ts` is indexed and
ranks first on a descriptive query; on a bare-symbol query, test and import
chunks out-BM25 the class body. The misses point at recall and ranking, not at
relationship traversal.

## Two changes

### 1. Signature-coverage signal in the reranker

`find_fts5_reranked` already rewards exact-phrase match and token coverage. This
slice adds a third lexical signal: how many query terms appear in the chunk's
leading (signature) line. A definition carries the queried symbol in its
signature (`export class RunStore`, `pub fn find`), while a reference buries it
in the body, so this lifts definitions above call sites.

The signal is gated to **identifier-shaped query terms only** (`RunStore`,
`chunk_record_from_row`, `FTS5`), reused via the crate-level `is_identifier_token`
helper that classification already relies on. Without that gate, the first line
of any prose paragraph earns a spurious boost and regresses natural-language
queries. Because `tokenize` discards case, the identifier set is derived from the
raw query before lowercasing.

The signal feeds the blended score only; it is deliberately not a hard sort key.
An earlier variant that made signature coverage a primary sort key (above token
coverage) regressed fts5-reranked from 47 to 44 by reordering prose results.

The reranker is the lexical spine of routed's symbol path (`find_hybrid` blends
by lexical rank), so the change lifts both strategies:

| strategy | before | after | tokens/answer |
| --- | --- | --- | --- |
| fts5-reranked | 47/68 | 48/68 | 1289.5 → 1279.9 |
| routed | 50/68 | 51/68 | 1295.7 → 1286.6 |

Both improved with no regressions, at slightly lower token cost. `RunStore` moved
from miss to rank 4 under routed.

### 2. Routed promoted to the CLI default

The CLI `find` and `eval` commands now default to `routed`; the other strategies
remain selectable with `--strategy`. The default-bearing path constructs the
embedding provider, so a default `find` now loads the model. Routed already falls
back to reranked FTS5 when a corpus has no embeddings, so the default degrades
gracefully.

## Scope boundary

The sync convenience API (`mycelia_core::find` / `find_headers`) and the stdio
**MCP server** still use reranked FTS5: they hold no embedding provider. Routing
the MCP surface requires a launch-loaded provider behind interior mutability (the
server derives `Clone` and the tool handlers take `&self`) plus a decision on
model-load-at-startup latency. That is the next slice, not this one.

## Verification

- 66 unit/integration tests pass, including two new reranker tests:
  `reranker_lifts_symbol_definitions_above_references` and
  `reranker_ignores_signature_line_for_prose_terms`.
- 68-case aggregate re-measured: fts5-reranked 48, routed 51.
- fmt, clippy (`-D warnings`), release build, and the stdio MCP exchange test all
  green.
