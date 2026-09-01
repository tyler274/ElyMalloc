#!/usr/bin/env bash
# Leptos (leptos-rs/leptos) WASM suites + in-tree reactive_graph smoke.
# Suites must run under wasmtime on wasm32-wasip1; compile success is not enough.
# LEPTOS_SRC=  LEPTOS_REFRESH=1
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- leptos "$@"
