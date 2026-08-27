#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/.." && pwd)"
cd "$ROOT"

cargo test -p mimalloc-core
cargo build --release -p mimalloc-c

SO="$ROOT/target/release/libmimalloc.so"
if [[ ! -f "$SO" ]]; then
  echo "missing $SO" >&2
  exit 1
fi

# SONAME and exported libc symbols
if command -v readelf >/dev/null; then
  readelf -d "$SO" | grep -E "SONAME" || true
fi
nm -D --defined-only "$SO" | grep -E " (malloc|free|calloc|realloc|posix_memalign|mi_malloc)$" >/dev/null

cc -O2 -pthread -o /tmp/mi-smoke "$ROOT/tests/smoke.c"
LD_PRELOAD="$SO" /tmp/mi-smoke

cc -O2 -pthread -DUSE_STD_MALLOC -DNDEBUG -I"$REPO/include" -o /tmp/mi-stress "$REPO/test/test-stress.c"
LD_PRELOAD="$SO" /tmp/mi-stress 4 10 3

ln -sfn libmimalloc.so "$ROOT/target/release/libmimalloc.so.3"
cc -O2 -pthread -DNDEBUG -I"$REPO/include" "$REPO/test/test-api.c" "$SO" -o /tmp/mi-api
LD_LIBRARY_PATH="$ROOT/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-api

cc -O2 -pthread -DNDEBUG -I"$REPO/include" "$ROOT/tests/theap.c" "$SO" -o /tmp/mi-theap
LD_LIBRARY_PATH="$ROOT/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-theap

echo "all rust mimalloc checks passed"
