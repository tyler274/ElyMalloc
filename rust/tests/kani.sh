#!/usr/bin/env bash
# Kani proofs for mimalloc-core and vma-core (integer models, no mmap/SIMD).
# No-ops if cargo-kani is not on PATH (`nix develop` or `cargo kani setup`).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
if ! cargo kani --version >/dev/null 2>&1; then
  echo "kani: cargo-kani not installed; skip"
  echo "install: nix develop   # or: cargo install --locked kani-verifier && cargo kani setup"
  exit 0
fi
cargo kani -p mimalloc-core
exec cargo kani -p vma-core
