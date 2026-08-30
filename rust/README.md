# Rust mimalloc rewrite

Pure-Rust allocator with a C ABI intended as a drop-in replacement for C mimalloc.

## Build

```
cd rust
cargo build --release -p mimalloc-c
```

This produces `target/release/libmimalloc.so` with SONAME `libmimalloc.so.3`.

`cargo check` is clean for `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-gnu`, `aarch64-unknown-linux-musl`, and `wasm32-unknown-unknown`. Musl cannot emit a `cdylib` unless `-C target-feature=-crt-static` is set (see `.cargo/config.toml`); `c_char` is `u8` on ARM, so path buffers use `libc::c_char` rather than `i8`.

## Test

```
cd rust
./tests/run.sh
# or: cargo test -p mimalloc-harness && cargo run -p mimalloc-harness -- run
```

Nix orchestrates the same suite (glibc and musl) as flake checks. Musl uses `rust-overlay` for `rust-std` so we do not rebuild rustc against musl:

```
nix flake check
# or individually:
nix build .#checks.x86_64-linux.glibc
nix build .#checks.x86_64-linux.musl
nix build .#mimalloc-musl
```

Compiler suites vs C mimalloc and stock jemalloc (needs `cmake`; `wasmtime` is not required):

```
./tests/oracle-suites.sh
# rustc UI only, still under Rust / C mimalloc / jemalloc:
SUITES=rustc ./tests/oracle-suites.sh
# same as: cargo run -p mimalloc-harness -- oracle
```

This builds C mimalloc with `MI_SECURE=FULL`, locates stock `libjemalloc.so` (`JEMALLOC_SO` or nixpkgs), runs the C ABI and C++ tests against the mimalloc libraries, then compiles GCC / Clang / rustc suite programs **once with the system toolchain** and runs those same binaries under `LD_PRELOAD` of each allocator. A test PASSes only if stdout, stderr, and exit code match a run of the same binary on the system malloc. FAIL sets must not grow vs C mimalloc or jemalloc. `./tests/compiler-preload.sh` is the Rust-only slice. Jemalloc skips GCC/Clang C torture unless `JEMALLOC_FULL=1`. Compiling rustc itself under the allocator (lld races) is a separate `rustc-lld-repeat` check.

## WASM

`mimalloc-core` targets `wasm32-unknown-unknown` and `wasm32-wasip1` with **no libc, C toolchain, or emscripten**. The OS layer grows linear memory (`memory.grow`); `munmap` cannot shrink it. Threads and `mprotect` guards are no-ops (single process heap).

```
rustup target add wasm32-unknown-unknown wasm32-wasip1
# also: cargo check -p mimalloc-core --target wasm32-unknown-unknown
./tests/wasm-smoke.sh
```

`wasm-smoke.sh` (or `cargo run -p mimalloc-harness -- wasm-smoke`) builds a `#[global_allocator]` program for both wasm targets, asserts the module does not import libc `malloc`, runs it under `wasmtime`, and runs `mimalloc-core` unit tests on `wasm32-wasip1`.

```rust
use mimalloc_core::Mimalloc;

#[global_allocator]
static ALLOC: Mimalloc = Mimalloc;
```

## NixOS

The flake overlay replaces `pkgs.mimalloc` with this library:

```nix
{
  inputs.mimalloc-rs.url = "path:/home/luluco/code/mimalloc";
  # and in nixos configuration:
  nixpkgs.overlays = [ mimalloc-rs.overlays.default ];
  environment.memoryAllocator.provider = "mimalloc";
}
```

`environment.memoryAllocator.provider = "mimalloc"` preloads `${pkgs.mimalloc}/lib/libmimalloc.so`.

## Secure mitigations

Always on (inspired by C `-DMI_SECURE=FULL`): encoded free lists, padding canaries, double-free detection, randomized page free lists, guard pages around page metadata **and at the end of every mimalloc page**, and ASLR-style gaps between OS mappings. Sampled object guard pages are off until `mi_theap_guarded_set_sample_rate`.

Release builds can enable C-style debug fill (`0xD0` / `0xDF` / `0xDE`) with `--features debug-fill` (off by default; it is always on in the debug profile).

## LD_PRELOAD compiler stress

`tests/compiler-preload.sh` (Rust crate `mimalloc-harness`) compiles GCC, Clang, and rustc suite programs with the system toolchain, then runs the **same binaries** with `LD_PRELOAD`. PASS means stdout, stderr, and exit status match the system malloc — compile success is not enough. Cases that already fail with the system malloc are skipped so only allocator regressions count. `tests/oracle-suites.sh` repeats the runs under C mimalloc (`MI_SECURE=FULL`) and stock jemalloc (same binaries) and requires the Rust FAIL set to be a subset of both. Harness filters and output comparison are unit-tested (`cargo test -p mimalloc-harness`).

## Later

- Compare **C mimalloc vs this Rust rewrite** as the process allocator for **Firefox, Chromium, and Electron** (startup, multi-process, and a short browsing/CI smoke on each).
