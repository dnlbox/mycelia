# User journey, observability, and onboarding

Implemented 2026-06-26. This document records the shipped slice `19` user
journey and observability surface.

## Why now

The retrieval engine is strong (routed: ~3.2x fewer tokens per answer than
reading the source cold, measured on the 68-case Forge gate). What decides
whether someone keeps Mycelia after trying it is not the engine but the journey
around it. Two questions, two audiences:

1. **"Is this saving me tokens?"** — the value question, asked by a power user
   evaluating cost.
2. **"How do I see it's running?"** — the trust question, asked by a non-terminal
   user who needs confirmation it works and is being used.

Before this slice, both were unanswerable from outside the process. Mycelia was a
headless stdio server: stdout was reserved for the MCP protocol, stderr was
invisible to the host, and there was no persistent record of activity or value.

## The three artifacts

One substrate (the log) feeds two surfaces (stats, status), deliberately split by
mental model: **stats answers "is it worth it?", status answers "is it working?"**

### Log — the substrate, and the non-terminal "is it alive" signal

A human-openable file in the data dir
(`~/.local/share/mycelia/logs/<corpus>.log`), not the terminal. One line per
meaningful event:

```
2026-06-26 09:14:02  serve start    corpus=forge  model=bge-small-en-v1.5  embeddings=current
2026-06-26 09:14:09  find           q="tenant lease context"  results=3  ~3.1x saved (1287 vs 4040 tok)
2026-06-26 09:15:21  retrieve       chunk=7aa4356  path=operio-agent/.../TenantHub.tsx
```

A newcomer double-clicks the file and sees activity, no terminal needed. Every
`find` line carries the token-savings estimate, so the log is also the raw
evidence behind `stats`. Logs are per corpus and bounded (rotate or cap size).

### `mycelia stats` — the tokenomics (value mental model)

Answers *"is this worth it?"* by aggregating the log:

```
corpus: forge
queries answered:        128
tokens via Mycelia:      ~164,700   (avg 1,287 / answer)
tokens if read cold:     ~523,000   (avg 4,087 / answer)
estimated savings:       ~3.2x  (358,300 tokens)
```

Forward-looking and persuasive: the "keep using it" story.

### `mycelia status` — the health (ops mental model)

Answers *"is it working / does it need attention?"*

```
corpus:            forge
index:             12,357 chunks
embeddings:        12,357 / 12,357 chunks  (current)
model:             fastembed-5.17.2:BAAI/bge-small-en-v1.5
last serve:        never
last refresh:      2026-06-26 20:52:57
db size:           50.4 MB
```

When something is wrong it says so and names the fix (`run mycelia refresh`).

## Path-aware command surface

The model is git: you are "in" a repo, and Mycelia infers the rest. Every
journey command works with zero arguments from inside a repository, with explicit
flags always available as an override.

### Resolution order (lowest effort wins, explicit always available)

1. `--corpus <name>` or `--database <path>` if given (power / diagnostic path).
2. Otherwise infer from the current directory: walk up to the **git root** and
   match it against registered corpus roots. With nested repositories, pick the
   **deepest** registered root that contains the current directory.
3. If the directory is under no registered corpus, fail with a message that names
   the fix (`run mycelia setup`).

`--database <path>` remains the escape hatch for fixture and diagnostic corpora
and never combines with a named or inferred corpus.

### Commands

The journey verbs are the entire user-facing surface. `serve` is internal,
invoked by the harness. The old `corpus set/show/list` subcommand group is
**retired**: its behaviour is absorbed by `setup` (set + index + embed), `status`
(show), and `list`. One way to do each thing.

| Command | Behaviour | Corpus source |
| --- | --- | --- |
| `mycelia setup [path] [--name N]` | Register + index + embed, with live progress. Default root = git root of cwd, name = its basename. Idempotent: re-running is a refresh. | cwd or `path` |
| `mycelia connect <harness>` | Wire the corpus into a harness (see below). | cwd or `--corpus` |
| `mycelia stats` | Tokenomics from the log. | cwd or `--corpus` |
| `mycelia status` | Index / embedding / model health. | cwd or `--corpus` |
| `mycelia refresh` | Forced full re-index + embed. A fallback, not the primary freshness mechanism: correctness is guaranteed at query time and the index is kept current in the background (see `20_freshness_and_staleness.md`). | cwd or `--corpus` |
| `mycelia list` | All registered corpora; mark the one matching cwd with `*`. | all |
| `mycelia delete` | Remove profile + database + embeddings + log, after showing what will be removed and confirming. | cwd or `--corpus` |
| `mycelia find <query>` / `retrieve <id>` | Manual inspection and debugging. | cwd / `--corpus` / `--database` |
| `mycelia eval <manifest>` | Measured evaluation (power use). | `--corpus` / `--database` |
| `mycelia serve` | Internal stdio MCP server, launched by the harness. | `--corpus` / `--database` |

Edge cases decided now:

- **Name collision** (two repositories both named `api`): fail and ask for an
  explicit `--name`, rather than silently namespacing. Predictable over clever.
- **`delete`** is destructive but local and reversible via re-`setup`, so an
  interactive confirmation showing the database path and size is sufficient.

## The onboarding journey

### The friction today

`brew install mycelia` lands a binary, then the path forks confusingly: run
`serve`? edit a JSON config? who builds the embeddings? The honest MCP model is
that **you never run `serve` by hand** in normal use; the harness spawns it.
`serve` is plumbing the harness calls, not a user command.

### The golden path

```
brew install mycelia
cd ~/forge
mycelia setup                  # registers `forge` -> ~/forge, indexes, embeds, shows progress
mycelia connect claude-code    # wires the corpus into the harness for you
# restart the harness -> it auto-launches `mycelia serve --corpus forge`
```

Under concept `22`, that single `connect` entry serves every registered corpus,
not just `forge`; the harness launches one multi-corpus server and the corpus is
resolved per request from the working directory.

Ongoing:

- `mycelia stats` — am I saving tokens?
- `mycelia status` — is it healthy?
- `mycelia refresh` — forced rebuild fallback only; freshness is otherwise
  automatic (query-time guarantee plus background refresh, see
  `20_freshness_and_staleness.md`).

### Three decisions this resolves

1. **Does the harness/server build embeddings on launch? No.** Setup is an
   explicit, observable terminal step. The server may *check* freshness and warn
   in the log, but must never block startup on a multi-minute embed: stdout is
   protocol-only (no progress channel to the host), so a silent first-query hang
   would be the worst possible first impression. The unembedded case already
   degrades to lexical retrieval gracefully.
2. **Who owns the server lifecycle? The harness.** Normal users only touch the
   journey verbs. `serve` stays an internal contract.
3. **`connect` is the highest-leverage onboarding feature.** Auto-wiring the
   harness removes the one step a non-terminal user cannot reasonably perform
   (hand-editing MCP configuration).

## connect: one core, several adapters

The spec emitted is identical everywhere: launch the stdio command
`mycelia serve --corpus <name>`. Only the file path and serialization differ, so
the design is one connector core that produces a server spec plus per-target
adapters that either call the harness's own CLI or write its config file.

> Superseded by concept `22_multi_corpus_server.md`. `connect` now writes **one**
> entry per harness, not one per corpus: a single multi-corpus server resolves
> the corpus per request (cwd default, explicit `corpus` only when the user names
> another project). `--corpus <name>` in the emitted spec becomes the *default*
> corpus for harnesses without a meaningful cwd (Claude Desktop), not the only
> one. Adding a corpus later needs no re-`connect`.

Principles:

- **Prefer the harness CLI** where one exists (for example `claude mcp add`):
  let the tool own its configuration file rather than hand-editing it.
- **Use the absolute path** to the `mycelia` binary in the emitted spec, because
  the harness process may not share the user's shell `PATH`.
- **Idempotent**: re-running `connect` updates the existing entry, never
  duplicates it.
- **Require a registered corpus**: `connect` errors (pointing at `setup`) if the
  corpus does not yet exist.

### First cut

| Target | Mechanism | Family |
| --- | --- | --- |
| Claude Code | `claude mcp add` CLI | prefer-CLI |
| Claude Desktop | `mcpServers{}` in `claude_desktop_config.json` | JSON-A |
| Codex CLI | `[mcp_servers.<name>]` in `~/.codex/config.toml` | TOML |
| Cursor | `mcpServers{}` in `~/.cursor/mcp.json` (low cost, reuses JSON-A) | JSON-A |

The JSON-A adapter covers Claude Desktop and Cursor from one code path. Exact
config paths and CLI flags for each target are verified at implementation time,
because these tools move quickly and may have changed.

### Later tiers

- **VS Code (Copilot)**: `.vscode/mcp.json` with a `servers{}` shape plus a
  `type` field; a near-cousin of JSON-A.
- **opencode**, **Antigravity**, and other agents: bespoke config; unverified.

## Personas served

- **Power-user evaluator**: runs `mycelia eval` on their own repository for a
  credible, data-grounded savings number; `stats` for the ongoing story.
- **Non-terminal newcomer**: `setup` then `connect` then restart; confirms life
  through the host's MCP server list, the tool calls appearing in the
  conversation, and the openable log.

## Implemented result

The query-time freshness guarantee (`20_freshness_and_staleness.md`, Layer 1)
landed ahead of everything here: not lying to the model outranks tokenomics.
Slice `19` then shipped:

1. **Log substrate** plus token-savings estimate on MCP `find`.
2. **`stats`** and **`status`** readers over the log and the database.
3. **Path-aware resolution** plus the flattened CLI (`setup`, `list`, `delete`,
   `refresh`; retired `corpus set/show/list`).
4. **`connect`** first cut: Claude Code CLI, Claude Desktop and Cursor via JSON,
   Codex via TOML.

Validation on the release binary:

- `setup` registered, indexed, and embedded a temporary fixture corpus.
- `connect codex` wrote an isolated `[mcp_servers.mycelia-<name>]` entry with an
  absolute binary path and stayed idempotent.
- A real stdio MCP exchange initialized the server, listed `find` and
  `retrieve`, called `find` against a temporary corpus, closed cleanly, and
  wrote `serve start` plus `find` lines to the corpus log.
- `stats` aggregated the logged query.
- Refreshed `forge`: 12,357 chunks, 12,357 embeddings, 50.4 MB database.
- Current 68-case gate on refreshed `forge`: routed 52/68 at 1413.0 tokens per
  answer, fts5-reranked 48/68 at 1450.7 tokens per answer.

## Deferred

- Menu-bar / tray app with a live "tokens saved" counter: the version that truly
  delights a non-technical user, but a real project, not a slice.
- `connect` for VS Code, opencode, Antigravity, and other harnesses.
- File watcher for embedding catch-up after query-time self-heal. It is now a
  latency optimization, not the correctness mechanism.
