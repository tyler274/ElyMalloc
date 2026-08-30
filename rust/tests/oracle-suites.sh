#!/usr/bin/env bash
# Compare Rust mimalloc with C mimalloc (SECURE=FULL) and stock jemalloc.
# SUITES=all|rustc  JEMALLOC_SO=  JEMALLOC_FULL=1
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- oracle "$@"
