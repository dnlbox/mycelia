# Evidence notes

This pass used local transcripts and repo artifacts. It is not a complete
academic history. It is a practical reconstruction of the journey.

## Sources checked

Local Codex sessions:

```text
/Users/titan/.codex/sessions/2026/06/25
/Users/titan/.codex/sessions/2026/06/26
/Users/titan/.codex/sessions/2026/06/27
/Users/titan/.codex/sessions/2026/06/28
```

Local Claude sessions:

```text
/Users/titan/.claude/projects/-Users-titan-forge-mycelia
```

Repo artifacts:

```text
AGENTS.md
BUILD_STATE.md
README.md
ROADMAP.md
docs/concept/
docs/concept/v2/
docs/evaluations/phase_b2_interactive_measurement.md
git log
```

Mycelia dogfood:

```text
target/release/mycelia status --database .mycelia/db/index.sqlite3
target/release/mycelia find --database .mycelia/db/index.sqlite3 ...
target/release/mycelia retrieve --database .mycelia/db/index.sqlite3 ...
```

The local project index was healthy during this pass:

```text
chunks: 2093
embeddings: 2093 / 2093
model: fastembed-5.17.2:BAAI/bge-small-en-v1.5
graph: 2263 edges over 388 symbols
last refresh: 2026-06-28 14:47:33
```

The named `mycelia` profile was not available in this shell, so dogfood used the
project-local database directly.

## Transcript coverage

The Codex search found 31 Mycelia-related rollout files across June 25 to June
28, 2026.

The Claude project folder contained 24 top-level Mycelia transcript files plus
subagent transcript material under the same project path.

The exact number is less important than the pattern:

- Codex carried much of the early implementation, review, distribution, and v2
  concept work.
- Claude carried many planning, critique, adoption, MCP, and harness-behavior
  probes.
- Both logs showed the same product lesson: tool availability does not guarantee
  tool use.

## High-signal transcript moments

- The project started with read-only product critique: compare Mycelia against
  Kilo, Antigravity, Codex, Claude Code, and Graphify before building.
- The first build authorization was explicit: take the lead, go as far as
  possible, follow the protocol, and keep docs in sync.
- The early slices used bounded subagents with disjoint file ownership.
- The team steered away from MCP client setup when retrieval quality still had
  obvious gaps.
- Graphify caused a real pause, then a bakeoff, then a decision to continue.
- `stats` and `status` emerged from user questions about beginner confidence:
  "is this saving tokens?" and "how do I know it is running?"
- Freshness became a hard requirement after the stale-code risk was called out.
- Homebrew planning clarified that install should not secretly download models
  or configure projects.
- MCP adoption testing showed that a harness can see Mycelia and still prefer
  native tools.
- Later audit tightened the rule: only visible MCP tool calls count as Mycelia
  use.

## Tool-use signal

Across the Claude Mycelia transcripts, the most common tool calls were still
native tools such as Bash, Edit, and Read. Mycelia MCP calls were present, but far
less frequent.

Observed Claude tool-call counts from the local transcripts included:

```text
Bash: 769
Edit: 427
Read: 351
mcp__mycelia__retrieve: 10
mcp__mycelia__locate_implementation: 8
mcp__mycelia__find: 6
mcp__mycelia-mycelia__find: 4
mcp__mycelia-mycelia__retrieve: 3
```

This is not a perfect measurement because transcript formats and harnesses differ.
It is still enough to support the core lesson: native tools have strong default
gravity.

Across the searched Codex rollout files, Mycelia-oriented tool names appeared,
but shell execution dominated there too. That matches the B2 measurement concern.

## Git history checkpoints

The git log shows the technical arc clearly:

- protocol scaffold
- local corpus indexing
- retrieval evaluation
- FTS5
- deterministic reranking
- read-only stdio MCP
- named corpus profiles
- semantic retrieval probe
- tree-sitter code chunking
- distilled `find` headers
- precision-first hybrid
- Graphify bakeoff
- query routing
- freshness and self-heal
- journey observability surface
- Homebrew distribution planning
- Ruby extraction
- multi-corpus MCP server
- duplicate header compaction
- typed graph edges
- v2 concept pack
- project-local config
- `mycelia init`
- guidance plane across harnesses
- Phase B2 interactive measurement protocol

This is why the retrospective frames Mycelia as a product journey, not a single
Rust implementation.

## Evidence caveats

- Local command messages can appear inside Claude transcripts as user messages.
  They were treated as evidence of local activity, not as normal conversation.
- Some sessions include interrupted turns or compacted summaries.
- Some Mycelia references are instructions or tool listings, not actual tool
  calls.
- Claimed Mycelia usage without a visible MCP tool-call record does not count as
  adoption.
- The retrospective intentionally avoids copying long transcript text. It
  summarizes the journey from repeated patterns and checked artifacts.
