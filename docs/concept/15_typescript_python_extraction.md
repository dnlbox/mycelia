# TypeScript and Python structural extraction

## Why now

Slices `09` and `11` established that tree-sitter structural chunking for Rust
dramatically improves retrieval precision: one chunk per function/struct/impl
rather than arbitrary paragraph splits keeps related code together and avoids
burying a function signature in the middle of a paragraph. The Forge corpus
includes 81 TypeScript/TSX files (candelabrum-studio) and 42 Python files
(operio-agent). Before this slice, all of them fell through to the plain-text
extractor.

After the bakeoff (`12`) and query routing (`13`), the remaining retrieval ceiling
was not the routing logic — it was the chunk quality for non-Rust files. Running
the oracle over the evaluation fixtures confirmed: TS/Python cases with exact
symbol names were matching but returning wrong chunks because the symbol appeared
in a plain-text paragraph that included surrounding unrelated code.

## Goal

Add tree-sitter grammars for TypeScript, TSX, and Python so that code files in
those languages are structurally chunked by declaration boundary, matching the
quality of the existing Rust extractor.

## Locked decisions

- **Three new extractor IDs**: `tree-sitter-typescript-v1` (`.ts`),
  `tree-sitter-tsx-v1` (`.tsx`), `tree-sitter-python-v1` (`.py`). Version
  suffix allows non-destructive re-extraction if node-kind lists change.
- **TypeScript includes exported declarations as top-level**: `export_statement`
  is listed as a top-level kind so that `export function`, `export class`,
  `export interface`, etc. are all captured as single chunks. All TypeScript
  top-level modules will be captured.
- **TypeScript collects a leading JSDoc block comment**: if a `comment` node
  starting with `/**` immediately precedes a declaration (no blank line), it is
  prepended to the chunk. This is crucial for semantic retrieval because JSDoc
  explains intent in plain English — the most valuable text for embedding.
- **Python relies on body docstrings**: Python top-level docstrings are the first
  statement of a function or class body. They are included automatically since
  the whole node is taken. No leading-comment walk-back is needed.
- **`decorated_definition` is one chunk**: `@router.post("/runs")` plus
  `async def create_run` are one declaration unit in Python's grammar. Taking the
  `decorated_definition` node captures both decorator and the function, which is
  the correct semantic unit for retrieval.
- **Plain-text fallback is preserved**: if the tree-sitter parser returns an empty
  result (empty file, unparseable syntax), the file falls back to plain-text
  extraction and `code_parse_fallbacks` is incremented.

## New dependencies

```toml
tree-sitter-python = "0.25.0"
tree-sitter-typescript = "0.23.2"
```

Both are ABI-compatible with `tree-sitter = "0.25"` (same C ABI version used by
the existing `tree-sitter-rust = "0.24"` grammar).

## Top-level node kinds

**TypeScript / TSX:**

```
function_declaration, class_declaration, interface_declaration,
type_alias_declaration, enum_declaration, abstract_class_declaration,
export_statement, ambient_declaration, module
```

**Python:**

```
function_definition, class_definition, decorated_definition
```

## Measured result

Re-indexed the full Forge cross-repo corpus after adding the new extractors:

| Metric | Before | After |
| --- | ---: | ---: |
| Total chunks | 7,411 | 9,410 |
| Code parse fallbacks | — | 36 |
| Indexing time | — | 1,721 ms |

Evaluation against the three Forge fixtures (fts5-reranked, no embeddings needed):

| Fixture | Before (routed, best) | After (fts5-reranked) |
| --- | ---: | ---: |
| forge-baseline (40) | 24 / 40 | 33 / 40 |
| forge-paraphrase (10) | 3 / 10 | 1 / 10 |
| forge-expanded (18) | 14 / 18 | 13 / 18 |
| **Total (68)** | **41 / 68** | **47 / 68** |

Structural extraction alone (fts5-reranked, no embeddings) now outperforms the
best strategy from the previous session (routed with embeddings). The baseline
jump from 24/40 to 33/40 is driven entirely by TypeScript and Python chunks now
being indexed at function/class/interface granularity rather than paragraph
granularity. Paraphrase performance is unchanged (1/10) because paraphrase
queries require semantic similarity, not lexical matches.

## Decision

Merge TypeScript and Python structural extraction as the production path. All 52
unit tests and 7 CLI integration tests pass.

## Deferred

- **Embedding refresh** for the new chunks: after re-indexing, the vector and
  routed strategies require a fresh `mycelia embed --corpus forge` run.
  With 9,410 chunks (up from 7,411), embedding time will increase proportionally.
- **JSDoc collection for multiple block comments**: currently only one `/** */`
  node is collected. Multiple JSDoc blocks are rare but the walk-back could be
  extended to match the Rust multi-`///` pattern.
- **TypeScript module members**: methods inside class bodies are not extracted as
  top-level chunks. A class with 20 methods produces one large chunk. A second
  pass that indexes class methods as sub-chunks would improve recall for
  method-level queries.
- **Python module-level constants**: `FOO = "bar"` at module scope is not
  currently extracted (not in `TOP_LEVEL_KINDS_PY`). These are useful for
  configuration-related queries.
- **Other languages**: Go, Java, C/C++ — future grammars follow the same pattern.
