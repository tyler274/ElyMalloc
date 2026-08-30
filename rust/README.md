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
```

Nix orchestrates the same suite (glibc and musl) as flake checks. Musl uses `rust-overlay` for `rust-std` so we do not rebuild rustc against musl:

```
nix flake check
# or individually:
nix build .#checks.x86_64-linux.glibc
nix build .#checks.x86_64-linux.musl
nix build .#mimalloc-musl
```

`./tests/compiler-preload.sh` is an extra GCC/Clang/rustc `LD_PRELOAD` slice (fetches compiler tests; not part of `nix flake check`).

## WASM

`mimalloc-core` targets `wasm32-unknown-unknown` and `wasm32-wasip1` with **no libc, C toolchain, or emscripten**. The OS layer grows linear memory (`memory.grow`); `munmap` cannot shrink it. Threads and `mprotect` guards are no-ops (single process heap).

```
rustup target add wasm32-unknown-unknown
cargo check -p mimalloc-core --target wasm32-unknown-unknown
```

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

`tests/compiler-preload.sh` rebuilds this library, then runs programs compiled by GCC, Clang, and rustc with `LD_PRELOAD=libmimalloc.so`. C torture cases that already abort with the system malloc are skipped so only allocator regressions count.

First slice (this machine): GCC `gcc.c-torture/execute` 1446/1446, Clang 1405/1405, plus host smoke/stress and `cargo test -p mimalloc-core` under preload. The full GCC/LLVM/rustc DejaGNU/`llvm-lit`/`x.py` suites are still later.

## Later

- Stress-test this library as a drop-in malloc (`LD_PRELOAD` / NixOS `memoryAllocator`) against the **full** GCC, LLVM, and rustc test suites (a first `LD_PRELOAD` slice lives in `tests/compiler-preload.sh`).
- Compare **C mimalloc vs this Rust rewrite** as the process allocator for **Firefox, Chromium, and Electron** (startup, multi-process, and a short browsing/CI smoke on each).
- Smoke the WASM `GlobalAlloc` path in a browser/`wasmtime` WASI program (the `memory.grow` backend already compiles for `wasm32-unknown-unknown`).
