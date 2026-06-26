# mycelia — vision

## Problem

Knowledge is scattered and re-read constantly. Across my own projects, premises
never flow (`operio-studio` → `operio-agent`). At the scale others work — a
Salesforce monorepo, a company of "100 repos" each a monorepo inside — no IDE can
navigate it. And the same need exists far outside code: a lawyer crossing several
hundred files of discovery against hundreds of thousands of jurisprudence
documents. The common shape: a corpus too large and too connected to hold in a head
or a context window.

## Thesis

A **memory you consult, not files you grep.** A content-agnostic knowledge graph,
local and owned, tuned for **precision and efficiency** against gnarly real
corpora. You ask `mycelia`; it returns the ranked, distilled, sourced slice — at a
fraction of the tokens of reading raw files.

## North stars

- **Precision** — a wrong connection is worse than none (decisive for law). Every
  edge is sourced and confidence-tagged.
- **Efficiency** — blazing fast, minimal footprint, never stale, never a full
  rescan.
- **Local + owned** — nothing leaves the box; no commercial dependency.

## Scale is content-shape, not file-count

One file of millions of lines and a tree of thousands of 500-line files are the
*same* problem once you index **symbols and ranges**, not files. Whole-file
indexing is what kills IDEs on big monorepos; sub-file, range-addressable nodes are
what don't. Design to the content shape, not a file count.

## Use-case spectrum (one core, many corpora)

- Personal: my code + `_notebook`, premises flowing between projects.
- Large monorepos / repos-of-repos: navigable where IDEs choke.
- Cross-corpus research: discovery × jurisprudence — any domain, not just code.

## Non-goals

- Code-only. (Code is one extractor; the core is content-agnostic.)
- A vendor cloud or a commercial product.
- Reinventing parsers (tree-sitter), embedding models, or document extractors.
- File-count benchmarks as the goal — precision + efficiency on real shapes is.
