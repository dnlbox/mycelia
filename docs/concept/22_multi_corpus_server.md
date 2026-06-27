# Multi-corpus MCP server

Spec, not yet implemented. This document defines the move from one stdio server
per corpus to a single server that resolves the corpus per request.

## Why now

`mycelia connect` wires one harness entry per corpus, because `serve` binds a
single database at launch (`serve(database, corpus_name, corpus_root, lexical)`
in `crates/mycelia-cli/src/mcp.rs`, building one `MyceliaServer::new(database,
provider, logger)`). Two costs follow:

1. **Reconnect friction.** Every new corpus means another `connect`, another
   harness config entry, another server process. The intended mental model was
   one MCP for Mycelia, not one per corpus.
2. **Tool-surface inflation feeds deferral.** Each server contributes its own
   `find` / `retrieve` / `locate_implementation` / `search_codebase`, so N
   corpora expose 4xN tools. Harnesses that defer MCP tools once the combined
   tool definitions cross a context-budget threshold (Claude Code's tool search
   activates at ~10% of the context window) are more likely to hide Mycelia
   entirely behind a search step the model never takes. Fewer tools keep Mycelia
   eagerly loaded and reachable.

A single server with an optional `corpus` argument collapses 4xN tools to ~5
total regardless of corpus count, removes the reconnect step, and keeps the
corpus invisible to the model in the common case.

## Evidence: deferral, not naming

Two fresh-Sonnet runs on the same orientation task (plan a Go tree-sitter
extractor), differing only in whether Mycelia's tools were loaded or deferred:

- **Deferred** (tools name-only behind the harness tool-search step): 6 grep/`ls`
  Bash calls, 5 `Read`, **zero** Mycelia calls. The model never searched for the
  tools; an always-loaded grep satisfied the need.
- **Loaded** (Mycelia-only MCP, eager): it **led** with Mycelia (5
  `locate_implementation` + 10 `retrieve`) and used `Read`/grep only afterward to
  confirm exact line ranges for the edit. Precisely the intended pattern: orient
  via the index, then read raw to edit.

The model reached for the `locate_implementation` alias, not bare `find`, so the
alias naming earns its keep, but only once the tool is loaded. The decisive lever
is presentation: keeping the combined tool surface small enough to stay eager
(this slice's 4xN to ~5 reduction) is what determines whether any of Mycelia's
adoption affordances reach the model at all.

## Design invariant

**The model names a corpus only when the user names one. Otherwise the corpus is
the current project, inferred.** The server mirrors the user's words: silent
in-project scope by default, explicit scope only when the user points at another
body of work ("check what we did in candelabrum and apply here"). The model never
chooses scope from a menu and never guesses across projects.

This is what rules out a search-everything default. Searching unnamed corpora
manufactures cross-project ambiguity (several React corpora each define
`login()`); the genuinely cross-corpus task is already covered by the user naming
the other corpus, which becomes a second, explicitly scoped call.

## Tool surface

Four tools gain an optional `corpus: Option<String>`; one new tool lists corpora.

| Tool | Change |
| --- | --- |
| `find` / `locate_implementation` / `search_codebase` | add optional `corpus` |
| `retrieve` | id carries its corpus (see below); `corpus` not needed |
| `list_corpora` | new; returns registered names + roots, marks the cwd default |

The find-family description is generated at startup from the live registry so the
model sees what is queryable without a call:

> Searches the current project's corpus by default. Pass `corpus` only to search
> a different project the user has named. Available corpora: mycelia, forge.

## Corpus resolution

Applied per request inside `MyceliaServer`:

1. `corpus` argument present, then that corpus.
2. Absent, then the **default corpus** = `infer_from_cwd(server_launch_cwd)`
   (`crates/mycelia-cli/src/profile.rs`: walk up to the nearest registered root,
   deepest ancestor wins). This is the 90% path and already exists.
3. Absent and cwd matches no root and exactly one corpus is registered, then that
   one.
4. Absent and cwd matches no root and several are registered, then return a cheap
   `needs_corpus` result listing names. Never a silent search-all.

Row 2 reuses the same resolver the journey CLI already uses (concept `19`,
path-aware command surface), so server and CLI agree on "which corpus am I in."

## Chunk ids carry their corpus

`chunk_id` is `blake3(source_path, source_hash, byte_range)` and is therefore only
unique within a corpus: two corpora can hold the same relative path. The MCP
surface namespaces ids as **`corpus:hash`** (for example `mycelia:6f0b...`).
`find` emits namespaced ids; `retrieve(chunk_id)` splits on the first `:` and
routes to that corpus' database, staying single-argument. The model keeps treating
the id as one opaque token, as today, with no chance of pairing a hash with the
wrong corpus. Headers may also expose a separate human-readable `corpus` field.

`retrieve`'s freshness path is unchanged except for routing: `validate_freshness`
(`crates/mycelia-core/src/store.rs`) already takes the corpus' `stored_root`, so
each corpus validates against its own root.

## Server internals

`MyceliaServer` moves from a single bound database to a lazy registry.

- **Registry source of truth is the existing profile store, read live per
  request.** A newly `setup` corpus is therefore reachable with no reconnect and
  no server restart; the cost is one cheap profile-list read per request.
- **Per-corpus handles are cached** as `name -> { database, root, provider:
  OnceCell }`. A corpus' embedding provider loads lazily on first query of that
  corpus, so launching with five corpora does not load five models, only the ones
  actually queried pay `FastEmbedProvider::load`.
- **Share the embedding model across corpora.** The `bge-small-en-v1.5` weights
  are identical for every corpus; only the per-chunk vectors differ by database.
  Factor the provider into a shared model plus a per-corpus vector store so the
  model loads once, not once per corpus. (Today `FastEmbedProvider::load(&db)`
  couples the two.)
- Logging stays per corpus (`log_path_for(name)`), keyed by the resolved corpus
  of each request rather than a single launch-bound name.

## serve and connect

- `serve` with no corpus, then multi-corpus with cwd-default resolution.
- `serve --corpus X`, then still multi-corpus, but **X is the default when cwd
  resolution fails**. This is the Claude Desktop answer: Desktop has no meaningful
  launch cwd, so `connect` for Desktop writes `--corpus X` as the fallback default
  while every other corpus remains reachable by name. This redefines `--corpus`
  from "the only corpus" to "the default corpus," a deliberate, back-compatible
  change: existing single-corpus configs keep working and simply gain reach to
  siblings.
- `connect` writes **one** server entry per harness instead of one per corpus.
  Adding a corpus later needs no `connect`. This supersedes the per-corpus
  `connect` first cut in concept `19`.

## Migration, mapped to files

1. `MyceliaServer` (`mcp.rs`): single database to lazy registry plus the per-call
   resolver calling `infer_from_cwd`.
2. Tool handlers (`mcp.rs`): add optional `corpus`; apply the resolution ladder;
   emit and parse `corpus:hash` ids.
3. `serve` signature and the `Command` enum (`mcp.rs`, `main.rs`): `--corpus`
   becomes the fallback default rather than the sole binding.
4. New `list_corpora` handler, reusing `profile::list`.
5. `connect` (concept `19` adapters): emit one entry; drop per-corpus
   registration; Desktop/Cursor JSON-A writes `--corpus <default>`.
6. Tests: extend `stdio_mcp_uses_named_corpus_and_calls_read_only_tools` for the
   cwd default, an explicit cross-corpus `corpus` argument, the `needs_corpus`
   disambiguation, and a `corpus:hash` round-trip through `retrieve`.

## Decided edge cases

- **No registered corpora at all.** `find`/`retrieve` return a structured error
  pointing at `mycelia setup`, matching the CLI's existing message.
- **Unknown corpus name passed.** Return `needs_corpus` with the available names
  rather than silently falling back to the default, so a typo is visible.
- **Name collision** is already prevented at `setup` (concept `19`: two repos
  named `api` must disambiguate with `--name`), so resolved names stay unique.

## Relationship to other slices

- Concept `19` (journey, `connect`, path-aware resolution): this slice extends
  `connect` to a single entry and reuses `infer_from_cwd`. The connect "first
  cut" table there is superseded by the single-entry model here.
- Concept `20` (freshness): unchanged; per-corpus `validate_freshness` already
  carries the right root.
- Concept `18` (routed MCP server): the shared-provider, route-by-default design
  is preserved; only the binding becomes per-request instead of per-launch.
