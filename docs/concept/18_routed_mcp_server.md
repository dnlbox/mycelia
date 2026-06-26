# Routed MCP server

## Why now

Slice `17` made `routed` the CLI default but explicitly left the stdio MCP server
on reranked FTS5, because the server held no embedding provider. The MCP surface
is the actual product: the tools an agent consults instead of re-reading files.
Leaving it lexical-only meant the measured routing win (51/68 vs 48/68, with
prose recall as the main gain) never reached the consumer that matters. This
slice routes the server.

## The obstacle and the shape of the fix

The rmcp tool router requires `MyceliaServer: Clone`, and tool handlers take
`&self`. The embedding provider needs `&mut self` for `embed_query`. So the
provider lives behind `Arc<Mutex<FastEmbedProvider>>`:

- `Arc` keeps the server cloneable for the router.
- `Mutex` grants the `&mut` that `embed_query` needs from an `&self` handler.

The field is `Option<SharedProvider>`: `Some` routes, `None` serves lexical-only.

```rust
type SharedProvider = Arc<Mutex<FastEmbedProvider>>;

struct MyceliaServer {
    database: PathBuf,
    provider: Option<SharedProvider>,
    tool_router: ToolRouter<Self>,
}
```

`find` locks the provider and calls `find_headers_with_embeddings` with
`RetrievalStrategy::Routed`; with no provider it calls the sync `find_headers`.
Routed already falls back to reranked FTS5 when the bound corpus has no
embeddings, so an unembedded index degrades gracefully at the query level too.

## Model load at startup

`serve` loads the embedding model once at startup rather than per query: an MCP
server is launched once and serves many calls, so a one-time load (a few hundred
ms warm, plus a first-run download) is the right trade. Two escape hatches keep
this from being a hard dependency:

1. **`serve --lexical`** skips the load entirely. Useful for air-gapped use, fast
   startup, or when embeddings are not wanted. This is what the integration test
   uses to stay offline.
2. **Graceful load failure**: if the model cannot load (for example, an
   air-gapped first run with no cached model), `serve` logs to stderr and serves
   lexical retrieval instead of refusing to start. Stdout stays reserved for the
   MCP protocol.

## Verification

- 66 tests pass. The stdio integration test runs `serve --lexical` end to end
  (initialize, `tools/list` returns `find`/`retrieve`, `tools/call`).
- Manual real exchange against the embedded Forge corpus with the default
  (routed) server: a prose query routed through the semantic path and returned a
  sourced header, confirming the model loads and routing engages.
- fmt, clippy (`-D warnings`), and release build green.

## Deferred

- A `tools/call` integration test against an embedded corpus is left out of the
  hermetic suite because it would require the model. The manual exchange covers
  it; a future fake-provider injection point could make it hermetic.
- Mutation tools, watchers, and federation remain deferred (unchanged).
