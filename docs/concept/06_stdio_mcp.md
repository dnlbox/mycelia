# Read-only stdio MCP vertical slice

## Goal

Make the existing Mycelia index usable from MCP-capable local AI clients without
coupling Mycelia to one harness or expanding the write boundary.

The first server surface is:

```text
mycelia serve --database <path>
```

It runs one MCP server over standard input and standard output and exposes:

- `find`: search the configured database with reranked FTS5
- `retrieve`: return one exact chunk by deterministic identifier

## Locked decisions

- Use the official Rust MCP SDK.
- Use stdio because Codex, Claude, OpenCode, Antigravity, and other local clients
  can launch the same executable without a resident daemon or network listener.
- Keep the database explicit at process launch. Client tool calls cannot select an
  arbitrary database.
- Keep this slice read-only. Index mutation, ignore changes, and corpus
  configuration require a later capability and consent design.
- Return structured JSON as tool content so every result preserves source path,
  byte range, line range, score, extractor, and chunk identifier.
- Keep `mycelia-core` synchronous. Tokio exists only in the CLI/MCP transport
  boundary because the official SDK requires an asynchronous runtime.
- Never emit logs or diagnostics to stdout while serving MCP. Standard output is
  reserved for protocol messages.

## Validation

The slice is complete when:

1. Existing format, strict Clippy, test, release-build, CLI-smoke, and Forge
   evaluation gates remain green.
2. A real stdio exchange initializes the server and lists exactly `find` and
   `retrieve`.
3. A tool call returns a sourced result from a temporary indexed corpus.
4. Closing client input terminates the server cleanly.

## Deferred

- `index` and `ignore` MCP tools
- per-client configuration generators
- file watching and automatic freshness
- streamable HTTP
- authentication and remote access
- resources, prompts, and typed graph queries
