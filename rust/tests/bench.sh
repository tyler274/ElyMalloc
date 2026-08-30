#!/usr/bin/env bash
# Wall-clock + user-instruction malloc comparison (glibc, rust, rust-secure,
# C mimalloc MI_SECURE=FULL, jemalloc, GlobalAlloc).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- bench "$@"
