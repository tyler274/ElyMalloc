#!/usr/bin/env bash
# Kani proofs for mimalloc-core (bin math, align_up, encode/decode).
# No-ops if cargo-kani is not installed (Kani is not in nixpkgs).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
if ! cargo kani --version >/dev/null 2>&1; then
  echo "kani: cargo-kani not installed; skip"
  echo "install: cargo install --locked kani-verifier && cargo kani setup"
  exit 0
fi
exec cargo kani -p mimalloc-core
