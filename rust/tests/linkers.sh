#!/usr/bin/env bash
# GNU ld, gold, LLVM LLD, mold, Wild + GlobalAlloc stress.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- linkers "$@"
