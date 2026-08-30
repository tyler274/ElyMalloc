#!/usr/bin/env bash
# Compile GCC/Clang/rustc suite programs once, run under LD_PRELOAD, match system-malloc output.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- compiler-preload "$@"
