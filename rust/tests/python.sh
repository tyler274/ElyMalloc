#!/usr/bin/env bash
# Run python/cpython Lib/test (regrtest) vs rewrite, C mimalloc, and libc.
# Suites must run under LD_PRELOAD / ld-nix.so.preload; compile success is not enough.
# PYTHON3=  PYTHON=  CPYTHON_SRC=  CPYTHON_REFRESH=1  CPYTHON_FULL=1  CPYTHON_TEST=
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"

if [[ -z "${PYTHON3:-${PYTHON:-}}" ]] && ! command -v python3 >/dev/null 2>&1; then
  if command -v nix >/dev/null 2>&1; then
    echo "==> resolving nixpkgs python3"
    PYTHON3="$(nix build --no-link --print-out-paths --inputs-from "$ROOT" nixpkgs#python3)/bin/python3"
    export PYTHON3
  fi
fi

cd rust
echo "==> cargo run -p mimalloc-harness -- python"
exec cargo run -q -p mimalloc-harness -- python "$@"
