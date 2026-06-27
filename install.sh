#!/bin/sh
set -eu

repo="${MYCELIA_REPO:-https://github.com/dnlbox/mycelia.git}"
ref="${MYCELIA_REF:-v0.1.4}"
ref_type="${MYCELIA_REF_TYPE:-tag}"
root="${MYCELIA_INSTALL_ROOT:-$HOME/.local}"
profile="${MYCELIA_CARGO_PROFILE:-release}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "mycelia installer: cargo is required. Install Rust first: https://rustup.rs/" >&2
  exit 1
fi

case "$ref_type" in
  tag | branch | rev) ;;
  *)
    echo "mycelia installer: MYCELIA_REF_TYPE must be tag, branch, or rev" >&2
    exit 1
    ;;
esac

case "$profile" in
  release)
    profile_args=""
    ;;
  debug)
    profile_args="--debug"
    ;;
  *)
    echo "mycelia installer: MYCELIA_CARGO_PROFILE must be release or debug" >&2
    exit 1
    ;;
esac

echo "Installing mycelia from $repo ($ref_type $ref) into $root"

# shellcheck disable=SC2086
cargo install \
  mycelia-cli \
  --git "$repo" \
  "--$ref_type" "$ref" \
  --root "$root" \
  --force \
  --locked \
  $profile_args

echo
echo "Installed: $root/bin/mycelia"
case ":$PATH:" in
  *":$root/bin:"*) ;;
  *)
    echo "Add this to your shell profile if needed:"
    echo "  export PATH=\"$root/bin:\$PATH\""
    ;;
esac
echo
echo "Next:"
echo "  cd <your-repo>"
echo "  mycelia setup"
