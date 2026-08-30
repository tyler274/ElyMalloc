#!/usr/bin/env bash
# Run NixOS-world packages against the rewrite.
# Prefers the flake check; also runs PATH probes via the harness.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"

if command -v nix >/dev/null 2>&1; then
  echo "==> nix build .#world-preload"
  nix build -L .#world-preload
fi

cd rust
echo "==> cargo run -p mimalloc-harness -- world"
exec cargo run -q -p mimalloc-harness -- world "$@"
