#!/usr/bin/env bash
# VMA 3.4 C ABI: virtual allocator, 3.4 symbols, Blender-style fake-Vulkan smoke.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"
exec cargo run -q -p mimalloc-harness -- vma "$@"
