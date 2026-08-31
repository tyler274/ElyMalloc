#!/usr/bin/env bash
# Firefox / Chromium / Electron as process-allocator smokes vs C mimalloc.
# Not a substitute for compiler-suite LD_PRELOAD. Uses unwrapped launchers
# and NixOS ld-nix.so.preload injection (bubblewrap), then checks child maps.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
cd "$ROOT"

if command -v nix >/dev/null 2>&1 && [[ "${NIX_BROWSERS:-}" == 1 ]]; then
  echo "==> nix build .#browsers-preload"
  nix build -L .#browsers-preload
fi

if [[ -z "${ELECTRON:-}" ]] && ! command -v electron >/dev/null 2>&1; then
  echo "==> resolving nixpkgs electron (not the libc-malloc wrap)"
  ELECTRON="$(nix build --no-link --print-out-paths --inputs-from "$ROOT" nixpkgs#electron)/bin/electron"
  export ELECTRON
fi

if [[ -z "${CHROMIUM:-}" ]] && ! command -v chromium >/dev/null 2>&1 && ! command -v microsoft-edge >/dev/null 2>&1; then
  echo "==> resolving nixpkgs chromium"
  CHROMIUM="$(nix build --no-link --print-out-paths --inputs-from "$ROOT" nixpkgs#chromium)/bin/chromium"
  export CHROMIUM
fi

cd rust
echo "==> cargo run -p mimalloc-harness -- browsers"
exec cargo run -q -p mimalloc-harness -- browsers "$@"
