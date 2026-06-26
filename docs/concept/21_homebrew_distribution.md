# Homebrew distribution

This is the distribution record for the first Homebrew-readiness slice. It keeps
Homebrew constraints separate from the user journey spec (`19`) while preserving
that spec's golden path.

## Why this needs its own spec

Homebrew is not only an install command. It decides what can happen at build
time, what native libraries should come from the package manager, what tests can
prove, and whether first-run behavior feels honest.

If these constraints stay buried in release notes, the project will drift toward
one of two bad outcomes:

1. `brew install mycelia` does too much: downloads opaque runtime artifacts,
   initializes models, or hides expensive work.
2. `brew install mycelia` does too little: installs a binary that needs manual
   environment variables or cannot run semantic retrieval after setup.

The target is the middle: install the tool and native runtime cleanly, then let
`mycelia setup` own corpus-specific indexing, model download, embedding, and
progress.

## User promise

The gold path from `19` remains the product contract:

```text
brew install mycelia
cd ~/forge
mycelia setup
mycelia connect claude-code
# restart the harness -> it auto-launches `mycelia serve --corpus forge`
```

Meaning:

- `brew install mycelia` installs the executable and any package-manager-owned
  native runtime dependencies. It does not register a corpus, build embeddings,
  download the embedding model, or mutate harness config.
- `mycelia setup` is the first expensive step. It registers the corpus, indexes
  it, downloads the model if needed, embeds with visible progress, and leaves the
  corpus ready for routed retrieval.
- `mycelia connect <harness>` writes or delegates MCP configuration.
- `mycelia serve --corpus <name>` is harness plumbing. It must never become the
  hidden first-run setup path.

## Packaging decisions

### 1. Prefer a tap first

Publish through a dedicated tap before attempting `homebrew/core`.

Reasons:

- A tap lets the project ship the gold path while the journey commands are still
  settling.
- `homebrew/core` adds notability, audit, and dependency-policy constraints that
  are easier to satisfy after real use.
- The formula can evolve quickly while `setup`, `connect`, `stats`, and `status`
  stabilize.

Core remains a later goal, not the first distribution gate. This source repo now
contains `Formula/mycelia.rb` so the formula can be tested and installed by URL,
but a true `brew install mycelia` journey still wants a dedicated tap repository.

### 2. Build from source

The formula should build the Rust CLI from a tagged source:

```ruby
class Mycelia < Formula
  desc "Local, content-agnostic knowledge index for agent retrieval"
  homepage "https://github.com/dnlbox/mycelia"
  url "https://github.com/dnlbox/mycelia.git", tag: "v0.1.0"
  head "https://github.com/dnlbox/mycelia.git", branch: "main"
  license "Apache-2.0"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/mycelia-cli")
  end
end
```

The release tag, `Cargo.toml` version, `Cargo.lock`, README, and LICENSE must
agree before publishing. A future tap-only formula can switch to a source archive
with a SHA-256 once the release process is split from the source repository.

### 3. Do not use ORT build-time downloads in the Homebrew build

The development build currently uses FastEmbed with ORT binary downloads enabled.
That is convenient locally but not a clean Homebrew story: a Rust build script
pulling native runtime binaries is harder to audit than a normal source build plus
a declared Homebrew dependency.

Homebrew distribution should instead use the packaged `onnxruntime` formula and
FastEmbed's dynamic ORT loading feature.

Target feature split:

```toml
[features]
default = ["semantic-download"]
semantic-download = [
  "fastembed/hf-hub-rustls-tls",
  "fastembed/ort-download-binaries-rustls-tls",
]
semantic-system-ort = [
  "dep:ort",
  "fastembed/hf-hub-rustls-tls",
  "fastembed/ort-load-dynamic",
  "ort/load-dynamic",
]
```

The Homebrew formula builds with:

```ruby
depends_on "onnxruntime"

def install
  system "cargo", "install",
    *std_cargo_args(path: "crates/mycelia-cli"),
    "--no-default-features",
    "--features", "semantic-system-ort"
end
```

Implemented feature names match the target. Homebrew builds must not rely on ORT
binary downloads.

### 4. Runtime ORT lookup must be invisible to users

Users should not have to export `ORT_DYLIB_PATH`.

Acceptable implementation options:

- Wrap the installed binary in the formula with `ORT_DYLIB_PATH` pointing at
  Homebrew's `onnxruntime` library.
- Add a Homebrew-specific runtime lookup path in the CLI when built with
  `semantic-system-ort`.
- Use a small launcher script that sets only the required runtime library path
  and then execs the real binary.

Implemented: both the formula wrapper and the `semantic-system-ort` CLI runtime
lookup are present. The wrapper sets `ORT_DYLIB_PATH` to Homebrew's
`onnxruntime` dylib while preserving a user override. The CLI also scans
`HOMEBREW_PREFIX`, `/opt/homebrew`, and `/usr/local` for the Homebrew
`onnxruntime` library before initializing FastEmbed.

The acceptance test is simple: after `brew install mycelia`, `mycelia setup`
can load ONNX Runtime without manual shell configuration.

### 5. The embedding model is setup-owned runtime data

Do not bundle the embedding model into the formula for the first Homebrew release.

Reasons:

- The model is not needed for install, lexical smoke tests, or basic command
  discovery.
- `mycelia setup` is already the explicit progress-bearing step for first corpus
  preparation.
- Bundling model data adds size, attribution, cache layout, and update coupling
  before the model choice is proven stable.

The model download is acceptable only in `setup` or explicit embedding commands,
not in `install`, `serve`, `find`, or `connect`.

Future option: add Homebrew `resource` blocks for model artifacts with fixed URLs
and SHA-256 checksums, install them under `pkgshare`, and teach Mycelia to use
that packaged model directory. Do this only if first-run model download becomes a
real adoption problem.

## Formula test path

The formula test must exercise real behavior without network access or model
downloads.

Use a lexical fixture:

```ruby
test do
  (testpath/"corpus").mkpath
  (testpath/"corpus/notes.txt").write("alpha beta answer\n")

  system bin/"mycelia", "index",
    testpath/"corpus",
    "--database",
    testpath/"index.sqlite3"

  output = shell_output("#{bin}/mycelia find 'alpha answer' " \
    "--database #{testpath}/index.sqlite3 --strategy fts5-reranked")

  assert_match "notes.txt", output
end
```

The test intentionally uses `fts5-reranked` so it proves the installed binary and
SQLite-backed index path without downloading embeddings.

## Release checklist

Before the first tap release:

1. Implement the `semantic-system-ort` build mode. (Done.)
2. Verify the installed binary finds Homebrew `onnxruntime` without user env vars.
3. Keep `setup` as the first model-download and embed path. (Done.)
4. Add or update README install instructions. (Done for source-repo formula.)
5. Tag the release.
6. Move or copy `Formula/mycelia.rb` to a dedicated tap before claiming
   `brew install mycelia`.
7. Run:

```text
brew install --build-from-source ./Formula/mycelia.rb
brew test mycelia
brew audit --strict --new --online ./Formula/mycelia.rb
```

8. Run the gold path manually against a disposable corpus:

```text
brew install mycelia
cd <fixture repo>
mycelia setup
mycelia status
mycelia connect claude-code
```

The release is not gold-path ready until `setup`, `status`, and the first
`connect` target exist.

## Deferred

- `homebrew/core` submission.
- Bottles and signed provenance.
- Packaged model resources under `pkgshare`.
- A cask or companion app for a tray/menu-bar UI.
- Non-macOS distribution packages.
