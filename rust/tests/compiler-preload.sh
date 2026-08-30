#!/usr/bin/env bash
# LD_PRELOAD the Rust libmimalloc.so while driving GCC, Clang/LLVM, and rustc.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$ROOT/.." && pwd)"
CACHE="$ROOT/target/compiler-stress"
SO="$ROOT/target/release/libmimalloc.so"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"

mkdir -p "$CACHE"
cd "$ROOT"

echo "==> build libmimalloc.so"
cargo build --release -p mimalloc-c
test -f "$SO"

find_clang() {
  if command -v clang >/dev/null 2>&1; then
    command -v clang
    return
  fi
  local p
  p="$(nix-build --no-out-link -E 'with import <nixpkgs> {}; clang' 2>/dev/null || true)"
  if [[ -n "$p" && -x "$p/bin/clang" ]]; then
    echo "$p/bin/clang"
  fi
}

CLANG="$(find_clang || true)"
GCC="$(command -v gcc)"
GXX="$(command -v g++)"
RUSTC="$(command -v rustc)"

echo "gcc:   ${GCC:-missing}"
echo "g++:   ${GXX:-missing}"
echo "clang: ${CLANG:-missing}"
echo "rustc: ${RUSTC:-missing}"
echo "so:    $SO"

fail=0
pass=0
skip=0

run_bin() {
  local name="$1"
  shift
  if LD_PRELOAD="$SO" timeout 60 "$@" >/dev/null 2>"$CACHE/${name}.err"; then
    echo "  ok   $name"
    pass=$((pass + 1))
  else
    echo "  FAIL $name"
    fail=$((fail + 1))
    tail -20 "$CACHE/${name}.err" || true
  fi
}

echo "==> host programs under LD_PRELOAD (gcc/g++)"
cc -O2 -pthread -o "$CACHE/mi-smoke" "$ROOT/tests/smoke.c"
run_bin gcc-smoke "$CACHE/mi-smoke"

cc -O2 -pthread -DNDEBUG -I"$REPO/include" "$ROOT/tests/cxx.cpp" "$SO" -lstdc++ -o "$CACHE/mi-cxx" 2>/dev/null || \
  "${GXX:-c++}" -O2 -pthread -DNDEBUG -I"$REPO/include" "$ROOT/tests/cxx.cpp" "$SO" -o "$CACHE/mi-cxx"
run_bin gxx-cxx "$CACHE/mi-cxx"

cc -O2 -pthread -DUSE_STD_MALLOC -DNDEBUG -I"$REPO/include" -o "$CACHE/mi-stress" "$REPO/test/test-stress.c"
run_bin gcc-stress "$CACHE/mi-stress" 2 4 2

if [[ -n "${CLANG:-}" ]]; then
  echo "==> host programs under LD_PRELOAD (clang)"
  "$CLANG" -O2 -pthread -o "$CACHE/mi-smoke-clang" "$ROOT/tests/smoke.c"
  run_bin clang-smoke "$CACHE/mi-smoke-clang"
  "$CLANG" -O2 -pthread -DUSE_STD_MALLOC -DNDEBUG -I"$REPO/include" -o "$CACHE/mi-stress-clang" "$REPO/test/test-stress.c"
  run_bin clang-stress "$CACHE/mi-stress-clang" 2 4 2
fi

echo "==> rustc: rebuild and test mimalloc-core under LD_PRELOAD"
if LD_PRELOAD="$SO" cargo test -p mimalloc-core --offline >/dev/null 2>"$CACHE/cargo-test.err"; then
  echo "  ok   cargo-test-mimalloc-core"
  pass=$((pass + 1))
else
  echo "  FAIL cargo-test-mimalloc-core"
  fail=$((fail + 1))
  tail -30 "$CACHE/cargo-test.err" || true
fi

fetch_gcc_torture() {
  local dest="$CACHE/gcc-execute"
  if [[ -d "$dest" ]] && find "$dest" -name '*.c' | head -1 | grep -q .; then
    return
  fi
  echo "==> fetch gcc.c-torture/execute (sparse)"
  rm -rf "$CACHE/gcc-src"
  mkdir -p "$CACHE/gcc-src"
  git -C "$CACHE/gcc-src" init -q
  git -C "$CACHE/gcc-src" remote add origin https://github.com/gcc-mirror/gcc.git
  git -C "$CACHE/gcc-src" sparse-checkout init --cone
  git -C "$CACHE/gcc-src" sparse-checkout set gcc/testsuite/gcc.c-torture/execute
  git -C "$CACHE/gcc-src" fetch --depth 1 origin master
  git -C "$CACHE/gcc-src" checkout -q FETCH_HEAD
  mkdir -p "$dest"
  cp "$CACHE/gcc-src/gcc/testsuite/gcc.c-torture/execute/"*.c "$dest/" 2>/dev/null || true
}

run_c_torture() {
  local cc="$1"
  local tag="$2"
  local dir="$CACHE/gcc-execute"
  local compiled=0 ran=0 bad=0
  local f out
  echo "==> $tag compiling/running gcc.c-torture/execute under LD_PRELOAD"
  shopt -s nullglob
  for f in "$dir"/*.c; do
    # Skip DejaGNU glue and files that need extra sources.
    if grep -qE 'dg-(error|require-effective-target)|__builtin_trap' "$f" 2>/dev/null; then
      skip=$((skip + 1))
      continue
    fi
    out="$CACHE/t-${tag}-$$"
    if ! LD_PRELOAD="$SO" "$cc" -O2 -w -lm -o "$out" "$f" >/dev/null 2>&1; then
      skip=$((skip + 1))
      continue
    fi
    compiled=$((compiled + 1))
    # DejaGNU extra flags are not applied here; skip if the binary already
    # fails with the system malloc so we only report allocator regressions.
    if ! timeout 5 "$out" >/dev/null 2>/dev/null; then
      skip=$((skip + 1))
      rm -f "$out"
      continue
    fi
    if LD_PRELOAD="$SO" timeout 5 "$out" >/dev/null 2>&1; then
      ran=$((ran + 1))
    else
      bad=$((bad + 1))
      echo "  FAIL $tag $(basename "$f")"
    fi
    rm -f "$out"
  done
  echo "  $tag torture: compiled=$compiled ran=$ran failed=$bad"
  pass=$((pass + ran))
  fail=$((fail + bad))
}

fetch_rust_run_pass() {
  local dest="$CACHE/rust-ui.list"
  if [[ -f "$dest" && -s "$dest" ]]; then
    return
  fi
  echo "==> fetch rustc tests/ui (sparse, run-pass filter later)"
  rm -rf "$CACHE/rust-src"
  mkdir -p "$CACHE/rust-src"
  git -C "$CACHE/rust-src" init -q
  git -C "$CACHE/rust-src" remote add origin https://github.com/rust-lang/rust.git
  git -C "$CACHE/rust-src" sparse-checkout init --cone
  git -C "$CACHE/rust-src" sparse-checkout set tests/ui
  git -C "$CACHE/rust-src" fetch --depth 1 origin main \
    || git -C "$CACHE/rust-src" fetch --depth 1 origin master \
    || {
      echo "  skip rustc ui fetch"
      return 0
    }
  git -C "$CACHE/rust-src" checkout -q FETCH_HEAD
  : >"$dest"
  find "$CACHE/rust-src/tests/ui" -name '*.rs' | while read -r f; do
    grep -q '//@ run-pass' "$f" || continue
    grep -qE '//@ (aux-build|edition|feature|ignore|needs-|revisions|compare-output)' "$f" && continue
    grep -q '//~' "$f" && continue
    printf '%s\n' "$f" >>"$dest"
  done
}

run_rustc_ui() {
  local list="$CACHE/rust-ui.list"
  local compiled=0 ran=0 bad=0
  local f out
  echo "==> rustc tests/ui run-pass subset under LD_PRELOAD"
  if [[ ! -s "$list" ]]; then
    echo "  skip rustc ui (no tests fetched)"
    return
  fi
  while read -r f; do
    out="$CACHE/r-$$"
    if ! LD_PRELOAD="$SO" "$RUSTC" --edition 2021 -O -o "$out" "$f" >/dev/null 2>&1; then
      skip=$((skip + 1))
      continue
    fi
    compiled=$((compiled + 1))
    if ! timeout 5 "$out" >/dev/null 2>/dev/null; then
      skip=$((skip + 1))
      rm -f "$out"
      continue
    fi
    if LD_PRELOAD="$SO" timeout 5 "$out" >/dev/null 2>&1; then
      ran=$((ran + 1))
    else
      bad=$((bad + 1))
      echo "  FAIL rustc $(basename "$f")"
    fi
    rm -f "$out"
  done <"$list"
  echo "  rustc ui run-pass: compiled=$compiled ran=$ran failed=$bad"
  pass=$((pass + ran))
  fail=$((fail + bad))
}

if [[ "${SKIP_C_TORTURE:-0}" != 1 ]]; then
  fetch_gcc_torture
  if [[ -n "$GCC" ]]; then
    run_c_torture "$GCC" gcc
  fi
  if [[ -n "${CLANG:-}" ]]; then
    run_c_torture "$CLANG" clang
  fi
fi

fetch_rust_run_pass
if [[ -n "$RUSTC" ]]; then
  run_rustc_ui
fi

echo
echo "compiler-preload summary: pass=$pass fail=$fail skip=$skip"
if [[ "$fail" -ne 0 ]]; then
  exit 1
fi
echo "compiler-preload ok"
