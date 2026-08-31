#!/usr/bin/env bash
# Run NixOS-world package tests against the rewrite vs C vs libc.
# Prefers the flake check; then PATH workloads from NIXOS_CONFIG=/etc/nixos.
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
