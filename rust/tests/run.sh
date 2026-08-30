#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/.." && pwd)"
cd "$ROOT"

export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cargo test -p mimalloc-core
cargo build --release -p mimalloc-c

SO="$ROOT/target/release/libmimalloc.so"
cargo build -p mimalloc-c
DEBUG_SO="$ROOT/target/debug/libmimalloc.so"

export SO INCLUDE="$REPO/include" C_TESTS="$ROOT/tests" UPSTREAM_TESTS="$REPO/test" DEBUG_SO
bash "$ROOT/tests/c-abi.sh"

echo "all rust mimalloc checks passed"
