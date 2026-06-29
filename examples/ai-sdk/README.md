# Mycelia AI SDK 7.0 Reference

This example verifies and demonstrates the Phase 3 integration surface:

- `createMCPClient` from `@ai-sdk/mcp`
- stdio transport through `Experimental_StdioMCPTransport`
- AI SDK tool objects with `inputSchema`
- `ToolLoopAgent` with `stopWhen: stepCountIs(...)`
- AI Gateway model IDs such as `anthropic/claude-sonnet-4-5`

Install dependencies:

```sh
npm ci
```

Run the no-model compatibility smoke:

```sh
MYCELIA_BIN="$HOME/.local/bin/mycelia" npm run smoke:mcp
```

Run the reference review agent after `mycelia ci prepare` has built the project-local index:

```sh
MYCELIA_PROJECT_ROOT="$PWD" npm run review -- src/file.ts src/other.ts
```

The review command calls a model through AI Gateway and requires the caller's normal AI SDK provider credentials. The smoke command does not call a model.
