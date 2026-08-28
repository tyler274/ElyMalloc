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

cc -O2 -pthread -DNDEBUG -I"$REPO/include" "$REPO/test/test-api-fill.c" "$SO" -o /tmp/mi-api-fill
LD_LIBRARY_PATH="$ROOT/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-api-fill

# Debug fill (`MI_DEBUG>=2`) only in the debug profile, matching C's debug memset.
cargo build -p mimalloc-c
DEBUG_SO="$ROOT/target/debug/libmimalloc.so"
ln -sfn libmimalloc.so "$ROOT/target/debug/libmimalloc.so.3"
cc -O0 -g -pthread -DMI_GUARDED=0 -I"$REPO/include" "$REPO/test/test-api-fill.c" "$DEBUG_SO" -o /tmp/mi-api-fill-debug
LD_LIBRARY_PATH="$ROOT/target/debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-api-fill-debug

cc -O2 -pthread -DNDEBUG -I"$REPO/include" "$REPO/test/test-stress-heaps.c" "$SO" -o /tmp/mi-stress-heaps
LD_LIBRARY_PATH="$ROOT/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-stress-heaps 4 10 3

cc -O2 -pthread -DNDEBUG -I"$REPO/include" "$REPO/test/test-stress-subprocs.c" "$SO" -o /tmp/mi-stress-subprocs
LD_LIBRARY_PATH="$ROOT/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-stress-subprocs 4 10 3

c++ -O2 -pthread -DNDEBUG -I"$REPO/include" "$ROOT/tests/cxx.cpp" "$SO" -o /tmp/mi-cxx
LD_LIBRARY_PATH="$ROOT/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-cxx

cc -O2 -pthread -DNDEBUG -I"$REPO/include" "$ROOT/tests/process.c" "$SO" -o /tmp/mi-process
LD_LIBRARY_PATH="$ROOT/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-process

cc -O2 -pthread -DNDEBUG -I"$REPO/include" "$ROOT/tests/secure.c" "$SO" -o /tmp/mi-secure
LD_LIBRARY_PATH="$ROOT/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/mi-secure

echo "all rust mimalloc checks passed"
