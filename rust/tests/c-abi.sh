#!/usr/bin/env bash
# C ABI / LD_PRELOAD checks. Env: SO, INCLUDE, C_TESTS, UPSTREAM_TESTS; optional DEBUG_SO, OUT.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- c-abi "$@"
