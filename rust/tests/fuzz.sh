#!/usr/bin/env bash
# Property tests + heap fuzzer + chaos monkey (core) and C ABI chaos.c.
# Default cargo test already runs a shorter budget; this raises MIMALLOC_CHAOS_STEPS.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- fuzz "$@"
