#!/usr/bin/env bash
# Install the `puddly` CLI onto PATH.
#
# Prefer a prebuilt GitHub Release binary (no Rust). Fall back to
# `cargo install --git` / local path only when no matching Release asset exists.
#
# From a clone:
#   ./install.sh
#
# One-liner (app authors — no clone):
#   curl -fsSL https://raw.githubusercontent.com/michaelmoses-op/Puddly/main/install.sh | bash
#
# Env:
#   PUDDLY_VERSION     pin a release tag/version (e.g. v0.1.0 or 0.1.0); default = latest
#   PUDDLY_INSTALL_DIR where to place the binary (default: ~/.local/bin)
#   PUDDLY_REPO        owner/repo (default: michaelmoses-op/Puddly)
#   PUDDLY_FORCE_CARGO=1  skip Releases and use Cargo
set -euo pipefail

REPO="${PUDDLY_REPO:-michaelmoses-op/Puddly}"
REPO_URL="${PUDDLY_REPO_URL:-https://github.com/${REPO}}"
INSTALL_DIR="${PUDDLY_INSTALL_DIR:-${HOME}/.local/bin}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || true)"

say() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; }

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux)  os=linux ;;
    Darwin) os=darwin ;;
    *) err "unsupported OS: $os (need Linux or macOS)"; exit 1 ;;
  esac
  case "$arch" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) err "unsupported arch: $arch"; exit 1 ;;
  esac
  if [[ "$os" == linux ]]; then
    echo "${arch}-unknown-linux-gnu"
  else
    echo "${arch}-apple-darwin"
  fi
}

normalize_version() {
  # strip leading v
  local v="$1"
  v="${v#v}"
  echo "$v"
}

latest_tag() {
  # Prefer the GitHub API; fall back to /releases/latest redirect.
  local tag
  if command -v curl >/dev/null 2>&1; then
    tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
      | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
      | head -n1 || true)"
    if [[ -z "$tag" ]]; then
      tag="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest" 2>/dev/null || true)"
      tag="${tag##*/}"
    fi
  fi
  if [[ -z "$tag" || "$tag" == "latest" ]]; then
    return 1
  fi
  echo "$tag"
}

install_from_release() {
  local target version tag asset url tmp stage
  target="$(detect_target)"
  if [[ -n "${PUDDLY_VERSION:-}" ]]; then
    tag="${PUDDLY_VERSION}"
    [[ "$tag" == v* ]] || tag="v${tag}"
  else
    tag="$(latest_tag)" || return 1
  fi
  version="$(normalize_version "$tag")"
  asset="puddly-${version}-${target}.tar.gz"
  url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

  say "Downloading ${asset} (${tag})…"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  if ! curl -fsSL "$url" -o "${tmp}/${asset}"; then
    err "no release asset at ${url}"
    return 1
  fi
  tar -C "$tmp" -xzf "${tmp}/${asset}"
  if [[ ! -f "${tmp}/puddly" ]]; then
    # tarball may nest a directory
    stage="$(find "$tmp" -type f -name puddly | head -n1 || true)"
    if [[ -z "$stage" ]]; then
      err "tarball missing puddly binary"
      return 1
    fi
    cp "$stage" "${tmp}/puddly"
  fi
  chmod +x "${tmp}/puddly"
  mkdir -p "$INSTALL_DIR"
  mv "${tmp}/puddly" "${INSTALL_DIR}/puddly"
  say "Installed: ${INSTALL_DIR}/puddly"
  return 0
}

install_from_cargo() {
  if ! command -v cargo >/dev/null 2>&1; then
    err "cargo not found — install Rust from https://rustup.rs, or wait for a GitHub Release binary."
    exit 1
  fi
  if [[ -n "${SCRIPT_DIR:-}" && -f "${SCRIPT_DIR}/crates/puddly-cli/Cargo.toml" ]]; then
    say "Installing puddly from local checkout (${SCRIPT_DIR}) via Cargo…"
    cargo install --path "${SCRIPT_DIR}/crates/puddly-cli" --locked --force
  else
    say "Installing puddly from ${REPO_URL} via Cargo (fallback)…"
    cargo install --git "$REPO_URL" --locked --force puddly-cli
  fi
  local cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
  if [[ -x "${cargo_bin}/puddly" ]]; then
    mkdir -p "$INSTALL_DIR"
    # Prefer a stable user bin; keep cargo copy too.
    ln -sfn "${cargo_bin}/puddly" "${INSTALL_DIR}/puddly" 2>/dev/null \
      || cp "${cargo_bin}/puddly" "${INSTALL_DIR}/puddly"
  fi
}

# --- main ---
if [[ "${PUDDLY_FORCE_CARGO:-}" == "1" ]]; then
  install_from_cargo
elif ! install_from_release; then
  say "No matching GitHub Release binary — falling back to Cargo…"
  install_from_cargo
fi

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    say "note: add ${INSTALL_DIR} to PATH, then re-open your shell:"
    say "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

if command -v puddly >/dev/null 2>&1; then
  say "Ready: $(command -v puddly) ($(puddly --version 2>/dev/null || echo ok))"
elif [[ -x "${INSTALL_DIR}/puddly" ]]; then
  say "Installed binary at ${INSTALL_DIR}/puddly (not yet on PATH)"
else
  cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
  say "Installed under ${cargo_bin}/puddly (ensure that dir is on PATH)"
fi

say
say "Next (in an app directory — Node 22+; Rust only if you used the Cargo fallback):"
say "  puddly init"
say "  # DATABASE_URL=…"
say "  puddly install && puddly migrate && puddly dev"
