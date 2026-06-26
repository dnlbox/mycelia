# Freshness and staleness

This is a design/vision document for work not yet implemented. It is not yet
reconciled into the AGENTS.md Project Specifics; that reconciliation happens when
the slices below are built.

## Why now

The index can drift from the repository, and nothing currently catches it at
query time:

- `retrieve` (`store.rs`) returns the stored chunk row verbatim. It never checks
  the file on disk.
- Content-hash (blake3) freshness exists, but is compared only during
  `index`/`refresh`, never during `find`/`retrieve`.

So if a repository changes and `refresh` has not run, the MCP server hands the
model **outdated code carrying authoritative provenance**: a path and line range
that no longer match reality. A confidently wrong answer is worse than no answer,
and this contradicts the project's standing precision-first decisions ("changed
unreadable source removes old results", "a false relationship is more harmful
than an absent one").

Making `refresh` a manual command the user must remember to run is therefore not
an acceptable freshness mechanism on its own. Two layers are needed, and the
correctness layer is non-negotiable.

## Layer 1 — Query-time freshness guarantee (must-have)

Make staleness fail loud, not silent. The data required is already stored
(`source_hash` per chunk), so no schema change is needed.

### `retrieve`

Before returning a chunk, re-validate it against disk:

1. `stat` the source file (cheap mtime/size check).
2. If that suggests a change, re-hash the file and compare to the stored
   `source_hash`.

Behaviour by outcome (precision over caching: only ever hand back content the
index is sure is real and current):

| Disk state | Action |
| --- | --- |
| Hash matches | Byte range still valid; return the precise indexed chunk (`ok`). |
| File changed | The indexed slice is no longer trustworthy, so read the whole current file live and return it (`file`). The caller gets up-to-date code, not a stale chunk, and does not need to be told it was stale. |
| File removed / unreadable / no longer text | Return an `unavailable` signal, never old text. |

Implemented (2026-06-26): the contract evolved from "always a chunk record" to a
tagged result (`ok` | `file` | `unavailable`). Rather than refusing a changed
source, `retrieve` falls back to the full live file so the answer stays correct.
The `unavailable` signal is a structured response the model can act on, for
example:

```json
{
  "status": "unavailable",
  "chunk_id": "7aa4356...",
  "source_path": "operio-agent/.../TenantHub.tsx",
  "message": "source no longer exists; re-run find or `mycelia refresh`"
}
```

`find` headers stay the discovery surface; `retrieve` is the point where the
freshness promise is enforced.

### Self-healing (implemented 2026-06-26)

When `retrieve` detects drift, the stdio MCP server quietly re-indexes that one
file in its launch-bound database (`refresh_source`): a changed file is
re-chunked in place, a removed or non-text file has its chunks pruned. This is
internal maintenance of the server's own database, not a model-facing mutation
tool, so the read-only tool surface and the no-arbitrary-paths rule both hold.
The user flow is never interrupted:

- The call still returns correct content (the live file) regardless of heal
  outcome.
- On success the heal is silent; later queries simply read fresh data.
- Only when the index cannot be repaired (for example a read-only filesystem)
  does the response carry a last-resort `refresh_hint` the harness can relay so
  the user runs `mycelia refresh`. Telling the user to run `refresh` is the
  exception, not the mechanism.

`find` enforces the same promise on its own surface: it validates the sources
behind its returned top-K against disk (`drifted_sources`), self-heals any
drifted file, and re-ranks once. The headers it returns therefore describe the
current files, and the `chunk_id`s it hands back resolve to fresh `ok` chunks on
`retrieve`. There is no "stale" flag on a header: a re-ranked header is simply
accurate. (One heal-and-re-rank pass is the bounded default; any residual drift a
second pass might catch is corrected by `retrieve` self-heal.)

Because the filesystem is the single source of truth and both surfaces validate
against it, `find` and `retrieve` cannot give contradictory answers: pruning a
removed source on `retrieve` also stops `find` from offering it. Re-indexed
chunks carry no embedding until the next embed pass; routed retrieval already
falls back to reranked FTS5 for them, so lexical and exact correctness are
immediate while the vector catches up.

### `find`

Annotate the top-K results with a freshness flag (bounded cost: K `stat` calls
per query, re-hash only when mtime indicates a change). Stale candidates are
flagged and ranked below fresh equivalents so the model is never silently handed
a stale header as authoritative. Strict callers may drop stale results entirely.

### Cost

One `stat` per `retrieve`, K per `find`; a re-hash only when mtime changed.
Negligible against embedding and query work. The guarantee means even a user who
refreshes once a week never receives wrong code: the worst case is a "stale,
re-run" signal.

## Layer 2 — Background freshness (convenience)

So that the answer to "do I run `refresh` often?" becomes "basically never, by
hand": a file watcher that debounces filesystem events and incrementally
re-indexes and re-embeds changed files while the server runs. This is the watcher
previously deferred; the freshness red flag justifies promoting it.

Cheaper interim steps, in order:

1. `find`/`retrieve` validate (Layer 1) and surface "index is N files stale; run
   `mycelia refresh`", turning silent drift into a visible nudge.
2. A periodic or query-burst-triggered incremental freshness scan (stat-based,
   re-indexing only changed files), bounded.
3. The full debounced watcher.

### Why Layer 1 is still required with a watcher

There is always a race window: a file can change between the last index and the
next query. Correctness cannot depend on the watcher having already caught up, so
query-time validation stays mandatory. The watcher only reduces how often Layer 1
trips.

## Embedding lag

A file re-indexed live (Layer 1 re-read, or Layer 2 incremental) will not have a
fresh embedding immediately. Routed retrieval already falls back to reranked FTS5
when a chunk has no embedding, so exact and lexical correctness is immediate while
the semantic vector catches up on the next embed pass. This is acceptable
graceful degradation, not a correctness hole.

## `refresh` re-framed

`mycelia refresh` is the manual, forced full rebuild: useful after a large
change, a model swap, or to recover from a corrupt state. It is a fallback, not
the primary freshness mechanism. Day to day, Layer 1 guarantees correctness and
Layer 2 keeps the index current without user action.

## Build order

1. **Layer 1 `retrieve` guarantee** (done) — precise chunk when fresh, whole live
   file when the source changed, `unavailable` otherwise. Never serve a stale
   slice. Implemented as precision-over-caching rather than refuse-on-stale.
2. **Server self-heal** (done) — on drift the server re-indexes/prunes the
   touched file in its bound DB (`refresh_source`); silent, never interrupts,
   manual `refresh` only as a last-resort hint.
3. **Layer 1 `find` validation** (done) — `find` validates the sources behind its
   top-K (`drifted_sources`), self-heals drift, and re-ranks once so headers are
   precise and agree with `retrieve`. No stale flag (superseded the original
   "annotate and deprioritize" plan).
4. **Layer 2 watcher** (deferred) — debounced incremental re-index and re-embed.
   Now only a latency optimization: query-time self-heal already guarantees
   correctness, so the watcher is no longer on the critical path.

## Deferred

- Cross-file structural staleness (a chunk that is byte-identical but whose
  meaning changed because a dependency changed) is out of scope; freshness here
  is per-source-file content identity.
- Distributed or remote corpora freshness is out of scope.
