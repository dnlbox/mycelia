# Visibility and diagnostics

## What users need to see

V2 should make adoption visible without requiring users to inspect logs by hand.

Questions to answer:

- Is this project initialized?
- Is the index present?
- Is it fresh enough?
- Did the current agent use Mycelia?
- Which files or symbols did Mycelia save the agent from rereading?
- Is CI restoring, rebuilding, or ignoring the index?
- What should I run next?

## `mycelia status`

Project-local `status` should report:

- project root,
- config path,
- DB path,
- schema version,
- chunk count,
- graph edge count,
- embedding coverage and model identity,
- last refresh,
- freshness problems,
- artifact/cache status when available,
- exact next fix.

Example:

```text
project:      my-project
config:       .mycelia/config.toml
index:        ready
db:           .mycelia/db/index.sqlite
chunks:       12,552
graph edges:  2,763
embeddings:   12,552 / 12,552
last refresh: 2026-06-27 22:10:12
next:         run mycelia stats --recent 20 to inspect agent usage
```

## `mycelia stats`

`stats` remains the adoption and token-value dashboard.

V2 requirements:

- default to the current project from `.mycelia/config.toml`,
- support `--all` for every discovered or registered project,
- show query counts,
- show recent `find` and `retrieve` activity,
- show tokens returned versus cold-read estimate,
- show zero-use language after init or connect,
- work from logs only when possible.

Zero-use wording:

```text
No Mycelia find calls recorded for this project yet.
Run a code-discovery task through your agent, then run:
  mycelia stats --recent 20
```

## CI summary

`mycelia ci prepare` should emit both human and machine summaries.

Human:

```text
Index restored from cache: yes
Refresh: 18 changed files, 0 rejected
Seed context: 9 headers, 3 likely tests
```

Machine:

```json
{
  "index_ready": true,
  "restored_from_cache": true,
  "files_refreshed": 18,
  "warnings": [],
  "mcp_command": ["mycelia", "serve", "--project", "/workspace/repo"]
}
```

## Dogfood evidence

Every Mycelia implementation slice still records:

```text
mycelia stats --corpus mycelia --recent 20
```

or the v2 equivalent:

```text
mycelia stats --project . --recent 20
```

If Mycelia was not used, the state file must say why. Valid reasons include
mandatory protocol file reads, exact line edits after Mycelia narrowed the area,
or a Mycelia miss.

## Debugging ladder

Preferred support flow:

1. `mycelia status`
2. `mycelia stats --recent 20`
3. `mycelia find "..." --json`
4. `mycelia retrieve <chunk-id> --json`
5. `mycelia ci prepare --dry-run` in CI projects
6. MCP stdio smoke only when harness integration is suspected

This keeps diagnostics close to the user journey and avoids adding a competing
`doctor adoption` command until the need is proven.

