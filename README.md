# mycelia

`mycelia` is a local, content-agnostic knowledge index written in Rust. It is
built for corpora that are too large or too connected to keep in a context window:
codebases, notes, protocol docs, discovery files, and eventually cross-corpus
research.

The thesis is simple: consult a sourced index instead of re-reading files.
`find` returns cheap, ranked headers; `retrieve` returns the exact body only after
the caller commits to a chunk. The project is local-first, precision-first, and
licensed as FOSS under Apache-2.0.

## Current status

The shipped baseline includes:

- deterministic discovery over ignored local files
- range-addressed UTF-8 chunks with byte ranges and one-based line ranges
- SQLite persistence with content-hash freshness
- tree-sitter chunking for Rust, TypeScript, TSX, Python, and Ruby
- reranked FTS5, vector, hybrid, and routed retrieval strategies
- local FastEmbed embeddings using `BAAI/bge-small-en-v1.5`
- named corpus profiles with derived local database paths
- manifest-driven retrieval evaluation with token-per-answer estimates
- a read-only multi-corpus stdio MCP server exposing `find`, aliases,
  `retrieve`, `find_related`, and `list_corpora`
- a deterministic depth-1 Rust `calls` graph: `graph` CLI command and
  `find_related` MCP tool, with conservative query-time name resolution
- query-time freshness validation plus MCP self-heal for drifted sources
- journey commands: `setup`, `connect`, `stats`, `status`, `refresh`, `list`,
  and `delete`
- per-corpus activity logs with token-savings estimates for `find`

The current Forge gate is 68 cases across baseline, expanded, and paraphrase
queries. Recent measured results on the refreshed local Forge corpus:

| Strategy | Hits | Tokens per answer |
| --- | ---: | ---: |
| `fts5-reranked` | 48 / 68 | 1395.9 |
| `routed` | 50 / 68 | 1391.9 |

The latest repairs exclude evaluation manifests from corpus discovery, collapse
exact duplicate chunk bodies in limited ranked headers, and add a conservative
Rust `calls` graph. `BUILD_STATE.md` records the remaining misses and gate
caveats.

`routed` is the CLI default and the MCP default when embeddings are available. It
falls back to reranked FTS5 when a corpus has no embeddings or the cached model is
unavailable. The provider-less synchronous core API remains lexical by default.

## Install

Target install, after Homebrew/core acceptance:

```text
brew install mycelia
```

Homebrew staging path for testing the same user experience before submitting to
Homebrew/core:

```text
brew tap dnlbox/mycelia
brew install mycelia
```

The tap repository should be `github.com/dnlbox/homebrew-mycelia`, with
`packaging/homebrew/Formula/mycelia.rb` copied to `Formula/mycelia.rb`.

Quick install with Cargo, no clone needed:

```text
cargo install --force mycelia-cli --git https://github.com/dnlbox/mycelia.git --tag v0.1.4 --locked
```

Curl installer, useful when you want one command and do not want to remember the
Cargo syntax:

```text
curl -fsSL https://raw.githubusercontent.com/dnlbox/mycelia/v0.1.4/install.sh | sh
```

The script installs the tagged CLI with Cargo into `${MYCELIA_INSTALL_ROOT:-$HOME/.local}`.
Override the version with `MYCELIA_REF`, for example:

```text
curl -fsSL https://raw.githubusercontent.com/dnlbox/mycelia/v0.1.4/install.sh | MYCELIA_REF=v0.1.4 sh
```

From a checkout, for development:

```text
cargo install --force --path crates/mycelia-cli --root "$HOME/.local"
```

The Homebrew formula builds from a tagged source archive with
`--no-default-features --features semantic-system-ort`, depends on
`onnxruntime`, and avoids ORT binary downloads during the formula build. The
embedding model is still downloaded only by `setup` or `embed`, never by
install, `find`, `serve`, or `connect`.

The repo's validation commands expect Cargo from the stable Rust toolchain path:

```text
env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --workspace --all-features
```

## Named corpus flow

Register a corpus once:

```text
cd ~/forge
~/.local/bin/mycelia setup
~/.local/bin/mycelia connect codex
~/.local/bin/mycelia status
~/.local/bin/mycelia stats
```

`setup` registers the corpus, indexes it, and embeds it with visible progress.
From inside a registered repository, most journey commands infer the matching
corpus from the current directory. Explicit `--corpus <name>` remains available.
`serve` is the harness-launched MCP command, not the normal user journey.

Profiles store only a canonical root:

```text
${XDG_CONFIG_HOME:-~/.config}/mycelia/corpora/<name>.json
```

Databases are derived from the profile name:

```text
${XDG_DATA_HOME:-~/.local/share}/mycelia/corpora/<name>.sqlite3
```

`MYCELIA_CONFIG_HOME` and `MYCELIA_DATA_HOME` override those homes for isolated
tests and development.

## Explicit path flow

Explicit paths remain available for fixtures, diagnostics, and automation:

```text
mycelia index <root> --database <path>
mycelia embed --database <path>
mycelia find <query> --database <path> [--strategy substring|fts5|fts5-reranked|vector|hybrid|routed]
mycelia retrieve <chunk_id> --database <path>
mycelia graph <symbol> --database <path> [--direction callers|callees]
mycelia eval <manifest> --database <path> [--strategy substring|fts5|fts5-reranked|vector|hybrid|routed]
mycelia serve --database <path> [--lexical]
```

A command accepts either a named corpus or explicit paths, never both.

## MCP surface

`serve` runs a read-only MCP server over stdio for local AI clients. In normal
named-corpus mode it is a single multi-corpus server: each request resolves the
corpus from an explicit `corpus` argument, the launch working directory, the
`--corpus` fallback default, or the sole registered corpus. Explicit
`--database` remains available for fixture and diagnostic servers. The MCP tools
are:

- `find`: ranked, sourced headers under a bounded result budget
- `search_codebase`: alias for `find`
- `locate_implementation`: alias for `find`
- `retrieve`: one selected chunk body by namespaced `corpus:hash` `chunk_id`
- `find_related`: callers or callees of a code symbol over the `calls` graph
- `list_corpora`: registered corpus names and roots for disambiguation

`find` does not return chunk bodies. A header includes path, byte range, line
range, score, extractor, `source_hash`, `chunk_id`, and a signature or synopsis.
`retrieve` re-reads the source file before returning a body. If the source changed,
it returns the whole current file live so the caller gets real, up-to-date code.
If the source vanished, escaped the corpus root, or cannot be verified as text, it
returns a structured `unavailable` signal. The MCP server also self-heals the
resolved corpus index for touched drifted files, so later `find` results converge
back to fresh headers.

In stdio mode, stdout is reserved for MCP protocol messages. Diagnostics go to
stderr. The server is intentionally read-only: indexing, ignore changes, mutation
tools, watchers, and arbitrary database selection are not exposed to the model.

## Retrieval model

The retrieval stack is evidence-gated:

- `substring` and raw `fts5` remain reference adapters.
- `fts5-reranked` is the lexical baseline and the fallback path.
- `vector` and `hybrid` remain selectable measured strategies.
- `routed` classifies the query locally and chooses a lexical-first or
  semantic-first profile.

Embeddings are cached per chunk with model identity and dimensions. `embed`
downloads the model on first use and then runs locally. Query and serve paths do
not trigger implicit model downloads; they degrade to lexical retrieval instead.

Evaluation changes must be judged on hit rate, MRR, and tokens per answered query.
Hit-rate gains that cost more answer tokens are not wins for the core use case.

## Prior art

[Graphify](https://github.com/safishamsi/graphify) is serious prior art for local
AST graphs and MCP-backed code navigation. The no-cost local bakeoff showed it is
valuable, especially around exact symbols and graph-neighborhood questions, but
it did not beat Mycelia on the current code-only structural gate.

That does not make Graphify irrelevant. It is a useful reference for graph
features, affected sets, and assistant integration. Mycelia stays separate because
the target is broader: range-addressed heterogeneous chunks, token-efficient
retrieval, explicit freshness guarantees, and eventual cross-corpus queries.

## Roadmap

Deferred work is tracked in `docs/concept/` and `BUILD_STATE.md`. The current
ordering is:

1. Refine and implement the v2 project-attached integration plan in
   `docs/concept/v2/`: `.mycelia/` project metadata, cwd-discovered MCP, CI
   prepare/seed flows, artifact/cache sharing, and consent-gated project
   instruction integration.
2. Ship concept `24` carry-forward items: `stats --all`, clearer zero-use
   signals, visible harness guidance where it remains useful, and slice closeout
   dogfood evidence.
3. Improve retrieval quality on the remaining 68-case Forge misses, but only if
   the token-per-answer gate holds.
4. Add a debounced watcher as a latency optimization for keeping embeddings
   current after query-time self-heal.
5. Extend the typed-edge graph (`23`): edges for TypeScript, Python, and Ruby;
   `imports`/`implements` edge types; method-call resolution via type
   information; and traversal beyond depth-1.
6. Add federation and specialized vector or storage layers only after local
   measurements justify them.

## License

Apache-2.0. This is the right default for this project if it should be FOSS and
usable by other agent tools, companies, and local workflows without copyleft
obligations.

The reason to prefer Apache-2.0 over MIT here is the explicit patent grant and
patent termination. MIT is simpler, but it is weaker for a project that may grow
into a shared indexing layer embedded in other tools. Strong copyleft licenses
would force more reciprocal sharing, but they would also reduce adoption for a
local CLI/library that should be easy to embed.
