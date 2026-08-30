#!/usr/bin/env bash
# C ABI / LD_PRELOAD checks against a built libmimalloc.
# Required env: SO, INCLUDE, C_TESTS, UPSTREAM_TESTS
# Optional: DEBUG_SO, OUT (defaults to TMPDIR or /tmp)
set -euo pipefail
CC="${CC:-cc}"
CXX="${CXX:-c++}"

SO="${SO:?SO=path/to/libmimalloc.so}"
INCLUDE="${INCLUDE:?INCLUDE=path/to/include}"
C_TESTS="${C_TESTS:?C_TESTS=path/to/rust/tests}"
UPSTREAM_TESTS="${UPSTREAM_TESTS:?UPSTREAM_TESTS=path/to/upstream/test}"
OUT="${OUT:-${TMPDIR:-/tmp}}"
mkdir -p "$OUT"

SODIR="$(cd "$(dirname "$SO")" && pwd)"
SO="$SODIR/$(basename "$SO")"
if [[ ! -f "$SO" ]]; then
  echo "missing $SO" >&2
  exit 1
fi

ln -sfn "$(basename "$SO")" "$SODIR/libmimalloc.so.3"

if command -v readelf >/dev/null; then
  readelf -d "$SO" | grep -E "SONAME" || true
fi
if command -v nm >/dev/null; then
  nm -D --defined-only "$SO" | grep -E " (malloc|free|calloc|realloc|posix_memalign|mi_malloc)$" >/dev/null
fi

"$CC" -O2 -pthread -o "$OUT/mi-smoke" "$C_TESTS/smoke.c"
LD_PRELOAD="$SO" "$OUT/mi-smoke"

"$CC" -O2 -pthread -DUSE_STD_MALLOC -DNDEBUG -I"$INCLUDE" -o "$OUT/mi-stress" "$UPSTREAM_TESTS/test-stress.c"
LD_PRELOAD="$SO" "$OUT/mi-stress" 4 10 3

"$CC" -O2 -pthread -DNDEBUG -I"$INCLUDE" "$UPSTREAM_TESTS/test-api.c" "$SO" -o "$OUT/mi-api"
LD_LIBRARY_PATH="$SODIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-api"

"$CC" -O2 -pthread -DNDEBUG -I"$INCLUDE" "$C_TESTS/theap.c" "$SO" -o "$OUT/mi-theap"
LD_LIBRARY_PATH="$SODIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-theap"

"$CC" -O2 -pthread -DNDEBUG -I"$INCLUDE" "$UPSTREAM_TESTS/test-api-fill.c" "$SO" -o "$OUT/mi-api-fill"
LD_LIBRARY_PATH="$SODIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-api-fill"

if [[ -n "${DEBUG_SO:-}" && -f "$DEBUG_SO" ]]; then
  DEBUGDIR="$(cd "$(dirname "$DEBUG_SO")" && pwd)"
  DEBUG_SO="$DEBUGDIR/$(basename "$DEBUG_SO")"
  ln -sfn "$(basename "$DEBUG_SO")" "$DEBUGDIR/libmimalloc.so.3"
  "$CC" -O0 -g -pthread -DMI_GUARDED=0 -I"$INCLUDE" "$UPSTREAM_TESTS/test-api-fill.c" "$DEBUG_SO" -o "$OUT/mi-api-fill-debug"
  LD_LIBRARY_PATH="$DEBUGDIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-api-fill-debug"
fi

"$CC" -O2 -pthread -DNDEBUG -I"$INCLUDE" "$UPSTREAM_TESTS/test-stress-heaps.c" "$SO" -o "$OUT/mi-stress-heaps"
LD_LIBRARY_PATH="$SODIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-stress-heaps" 4 10 3

"$CC" -O2 -pthread -DNDEBUG -I"$INCLUDE" "$UPSTREAM_TESTS/test-stress-subprocs.c" "$SO" -o "$OUT/mi-stress-subprocs"
LD_LIBRARY_PATH="$SODIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-stress-subprocs" 4 10 3

"$CXX" -O2 -pthread -DNDEBUG -I"$INCLUDE" "$C_TESTS/cxx.cpp" "$SO" -o "$OUT/mi-cxx"
LD_LIBRARY_PATH="$SODIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-cxx"

"$CC" -O2 -pthread -DNDEBUG -I"$INCLUDE" "$C_TESTS/process.c" "$SO" -o "$OUT/mi-process"
LD_LIBRARY_PATH="$SODIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-process"

"$CC" -O2 -pthread -DNDEBUG -I"$INCLUDE" "$C_TESTS/secure.c" "$SO" -o "$OUT/mi-secure"
LD_LIBRARY_PATH="$SODIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$OUT/mi-secure"

echo "c-abi checks passed"
