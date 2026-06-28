# Timeline

This is the lean version of the Mycelia journey. Dates are local project dates
from git history, Codex sessions, Claude sessions, and `BUILD_STATE.md`.

## 2026-06-25: from idea to first system

The first serious discussion was not "build a Rust tool." It was product
framing:

- Should this be its own thing, or should we just adopt tools like Kilo?
- Is Graphify close enough?
- Is Mycelia actually different?

The answer was: build only if Mycelia is a broader local memory layer, not just a
code graph clone.

The first implementation slice stayed boring on purpose:

- local file discovery
- UTF-8 chunks with ranges
- deterministic chunk ids
- SQLite persistence
- `index`, `find`, and `retrieve`
- fixture and CLI smoke tests

That choice mattered. It made the project testable before adding embeddings,
graphs, or MCP.

## 2026-06-25: retrieval became measured

The project moved from "it feels useful" to manifest-driven evaluation.

Early measured steps:

- substring baseline: weak but simple
- raw FTS5: better lexical retrieval
- deterministic reranking: strong enough to become default

The important cultural shift was the metric: hit rate was not enough. Mycelia
started judging retrieval by tokens per answered query. A retrieval change that
finds more hits but makes the model read more text is not a win.

## 2026-06-25 to 2026-06-26: MCP became real

The first MCP surface was intentionally read-only:

- `find`: discover ranked context
- `retrieve`: fetch one exact chunk

That two-stage shape became one of the strongest project ideas. It avoids the
common failure mode where a search tool dumps too much text into the model.

Later, `find` was changed to return headers instead of bodies. This made the MCP
surface more token-efficient and forced the model to commit before reading full
content.

## 2026-06-26: semantic search helped, but did not win

Embeddings and hybrid retrieval were tested because exact lexical search missed
paraphrased intent.

The result was nuanced:

- vectors helped some paraphrase cases
- the hybrid improved some hit counts
- token-per-answer got worse in key cases

So reranked FTS5 stayed the default. This was a good engineering moment: the
team did not ship the cooler technique just because it felt more advanced.

## 2026-06-26: code-aware chunks changed the game

Tree-sitter entered the project after the team realized chunk quality was a
bigger lever than ranking tricks.

The project added code-aware extraction for languages like Rust, TypeScript,
Python, and Ruby. That made chunks line up with real symbols instead of arbitrary
text blocks.

This was a healthy reversal. Earlier, tree-sitter was deferred. Measurement made
it worth adding.

## 2026-06-26: Graphify was a serious check

Graphify was not dismissed. It was treated as real prior art.

The local no-cost bakeoff showed:

- Graphify has useful AST graph ideas.
- Mycelia was still stronger on the current code-only structural gate.
- A full Graphify comparison would require explicit backend and model-spend
  approval.

The lesson: competitor research should sharpen the product, not create panic.

## 2026-06-26: the beginner journey appeared

The project stopped being only a diagnostic CLI.

The user-facing journey became:

```text
mycelia setup
mycelia connect
mycelia status
mycelia stats
mycelia refresh
mycelia list
mycelia delete
```

This was a product step. `stats` answered "is this saving tokens?" and `status`
answered "is it working?"

That mental model was much better than making beginners think in databases,
corpus profiles, and MCP internals.

## 2026-06-26: freshness became a trust contract

A red flag came up: what if Mycelia returns a chunk from old code?

The answer became a hard rule:

- return the precise chunk only if the source still matches
- if the source changed, return the live file instead
- if the source is gone or unreadable, say it is unavailable
- self-heal the index when the MCP server detects drift

This became the "do not lie to the model" guarantee.

## 2026-06-26 to 2026-06-27: packaging exposed product reality

Installation forced sharper thinking.

The desired gold path was:

```text
brew install mycelia
mycelia setup
mycelia connect
```

Homebrew constraints made hidden downloads and ONNX Runtime packaging risks
visible. The project kept model downloads in `setup`, not install.

That protected the user journey: install should install the tool, not secretly
index projects or download model assets.

## 2026-06-27: MCP availability failed as adoption

This was the biggest lesson.

Mycelia was connected. The server was running. The tools existed. Still, fresh
agent sessions often used shell tools like grep or read instead of Mycelia.

The uncomfortable finding:

```text
MCP availability is not adoption.
```

One audited Codex thread even claimed Mycelia use in prose while the raw transcript
showed shell commands instead of Mycelia MCP calls.

That forced a better measurement rule: only transcript-visible Mycelia MCP tool
calls count as use.

## 2026-06-27: one MCP, many corpora

The first connection model created one server entry per corpus. That was not the
right mental model.

The project moved toward one generic Mycelia MCP server that resolves the current
project from context. This reduced harness clutter and made the product feel more
like an installed capability than a pile of per-project servers.

## 2026-06-27: graph edges were added carefully

The first typed graph slice added Rust `calls` edges.

The key design choice was conservative resolution:

- store callee names
- resolve at query time
- drop unknown external names
- mark ambiguous matches
- avoid method-call resolution without type information

The rule was simple: a missing edge is better than a wrong edge.

## 2026-06-27 to 2026-06-28: v2 pivot

The project pivoted from user-level setup to project-attached context.

The new boundary became `.mycelia/`:

- config
- local database
- logs
- cache
- guidance fragments
- optional artifacts

The reason was practical: teams and CI need repo-owned context. User-level magic
is convenient for solo use, but it is too invisible and too hard to reason about
in shared environments.

## 2026-06-28: guidance became a first-class plane

The project learned that `.mycelia/AGENTS.md` alone is not enough if a harness
does not read it or if the model still prefers native tools.

So v2 split the product into three planes:

- index plane
- guidance plane
- connection plane

The guidance plane exists because the model must actually see the instruction at
the moment it chooses tools.

## 2026-06-28: the current gate

The project is now in Phase B2 measurement.

The real question is no longer "can Mycelia retrieve good chunks?" It can.

The question is:

```text
Will Codex and Claude Code choose Mycelia first, and does that reduce tokens to
the right files?
```

If yes, continue v2. If no, narrow the product or shelf the interactive-harness
path.
