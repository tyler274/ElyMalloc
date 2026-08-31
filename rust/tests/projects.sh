#!/usr/bin/env bash
# Run oven-sh/bun and serde-rs/serde test suites vs rewrite, C mimalloc, and libc.
# Suites must run under LD_PRELOAD / ld-nix.so.preload; compile success is not enough.
# PROJECTS=bun|serde|all  BUN=  BUN_SRC=  SERDE_SRC=  BUN_FULL=1  BUN_TEST=
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"

if [[ -z "${BUN:-}" ]] && ! command -v bun >/dev/null 2>&1; then
  if command -v nix >/dev/null 2>&1; then
    echo "==> resolving nixpkgs bun"
    BUN="$(nix build --no-link --print-out-paths --inputs-from "$ROOT" nixpkgs#bun)/bin/bun"
    export BUN
  fi
fi

cd rust
echo "==> cargo run -p mimalloc-harness -- projects"
exec cargo run -q -p mimalloc-harness -- projects "$@"
