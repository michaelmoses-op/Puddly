#!/usr/bin/env bash
# Install the `puddly` CLI onto PATH (via Cargo).
#
# From a clone:
#   ./install.sh
#   # or: cargo install --path crates/puddly-cli --locked --force
#
# One-liner (requires Rust/Cargo on PATH):
#   curl -fsSL https://raw.githubusercontent.com/michaelmoses-op/Puddly/main/install.sh | bash
set -euo pipefail

REPO_URL="${PUDDLY_REPO_URL:-https://github.com/michaelmoses-op/Puddly}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found — install Rust from https://rustup.rs then re-run." >&2
  exit 1
fi

if [[ -f "$SCRIPT_DIR/crates/puddly-cli/Cargo.toml" ]]; then
  echo "Installing puddly from local checkout ($SCRIPT_DIR)…"
  cargo install --path "$SCRIPT_DIR/crates/puddly-cli" --locked --force
else
  echo "Installing puddly from $REPO_URL…"
  cargo install --git "$REPO_URL" --locked --force puddly-cli
fi

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
if ! command -v puddly >/dev/null 2>&1; then
  echo "note: puddly is not on PATH yet — add $CARGO_BIN to PATH, then re-open your shell."
fi

if command -v puddly >/dev/null 2>&1; then
  echo "Installed: $(command -v puddly) ($(puddly --version 2>/dev/null || echo ok))"
else
  echo "Installed binary under $CARGO_BIN/puddly"
fi

echo
echo "Next (in an app directory):"
echo "  puddly install   # Node 22+ doctor + build"
echo "  puddly migrate   # apply migrations (needs DATABASE_URL)"
echo "  puddly dev"
