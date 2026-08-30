#!/usr/bin/env bash
# libc-less WASM GlobalAlloc smoke: wasm32-unknown-unknown + wasm32-wasip1.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- wasm-smoke "$@"
