# Named corpus profiles and local installation

## Goal

Give local clients one stable executable path and one stable corpus name:

```text
~/.local/bin/mycelia serve --corpus forge
```

Client configuration must not embed a repository checkout path or a database
path. The Forge root remains machine-local configuration.

## Profile model

A profile stores one canonical corpus root. Its database path is derived from the
profile name:

```text
config: ${XDG_CONFIG_HOME:-~/.config}/mycelia/corpora/<name>.json
data:   ${XDG_DATA_HOME:-~/.local/share}/mycelia/corpora/<name>.sqlite3
```

`MYCELIA_CONFIG_HOME` and `MYCELIA_DATA_HOME` override the complete Mycelia
directories for isolated tests and development.

Profile names use ASCII letters, digits, `_`, and `-`; they cannot begin with
punctuation. This keeps names safe as filenames and command-line identifiers.

## CLI

```text
cargo install --path crates/mycelia-cli --root "$HOME/.local"

cd ~/forge
mycelia setup --name forge
mycelia status --corpus forge
mycelia list
mycelia find "query" --corpus forge
mycelia retrieve <chunk_id> --corpus forge
mycelia eval <manifest> --corpus forge
mycelia serve --corpus forge
```

The existing explicit root and database forms remain available for temporary
fixtures, diagnostics, and automation. A command accepts either a named corpus
or explicit paths, never both.

## Locked decisions

- Store profiles as small JSON files rather than introducing a configuration
  database.
- Store only the canonical corpus root; derive the database location.
- Keep profile mutation in the local CLI. MCP remains read-only.
- Do not hard-code a Forge checkout path in the binary or repository.
- Do not add per-client configuration generators in this slice.

## Validation

The slice is complete when:

1. Existing format, strict Clippy, test, release-build, CLI-smoke, Forge
   evaluation, and MCP gates remain green.
2. An isolated profile home can register `forge`, index it, query it, and serve
   it without an explicit database path.
3. Invalid profile names and mixed explicit/profile targets fail clearly.
4. The README documents installation to `~/.local/bin` and the named Forge
   setup.
