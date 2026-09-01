# Rust mimalloc rewrite

Pure-Rust allocator with a C ABI intended as a drop-in replacement for C mimalloc **v3.5.1**, plus a pure-Rust AMD VMA **3.4** drop-in.

Crate rustdocs (`//!` / `///`) are the per-module source of truth. This README is the operator map (build, NixOS, harness).

## Crates

| Crate | Role |
|-------|------|
| `mimalloc-core` | `no_std` allocator (pages, heaps, arenas). Always-on `MI_SECURE` mitigations. `GlobalAlloc` via `Mimalloc`. |
| `mimalloc-c` | `cdylib`/`staticlib`: libc + `mi_*`, SONAME `libmimalloc.so.3` or `libmimalloc-secure.so.3`. |
| `mimalloc-harness` | Oracle, world, browsers, Bun/Serde, VMA, wasm. Logic is unit-tested in the lib. |
| `vma-core` / `vma-c` | AMD VMA 3.4 C ABI (`libVulkanMemoryAllocator.so.3`). |
| `mimalloc-wasm-smoke` / `mimalloc-alloc-stress` / `mimalloc-bench` | GlobalAlloc smokes and benches. |

## Build

```
cd rust
cargo build --release -p mimalloc-c
```

This produces `target/release/libmimalloc.so` with SONAME `libmimalloc.so.3`. `cargo build --release -p mimalloc-c --features secure` produces the same mitigations with SONAME `libmimalloc-secure.so.3` (C `-DMI_SECURE=ON` / `FULL`). The harness copies that to `target/release/libmimalloc-secure.so`.

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
# rebuild mold with this library statically linked:
nix build .#mold
nix build .#checks.x86_64-linux.mold
nix build .#vma
nix build .#checks.x86_64-linux.vma
nix build .#browsers-preload
```

Compiler suites vs C mimalloc and stock jemalloc (needs `cmake`; `wasmtime` is not required):

```
./tests/oracle-suites.sh
# rustc UI only, still under Rust / C mimalloc / jemalloc:
SUITES=rustc ./tests/oracle-suites.sh
# same as: cargo run -p mimalloc-harness -- oracle
```

This builds C mimalloc with `MI_SECURE=FULL`, locates stock `libjemalloc.so` (`JEMALLOC_SO` or nixpkgs), runs the C ABI and C++ tests against the mimalloc libraries, then compiles GCC / Clang / rustc suite programs **once with the system toolchain** and runs those same binaries under `LD_PRELOAD` of each allocator. A test PASSes only if stdout, stderr, and exit code match a run of the same binary on the system malloc. FAIL sets must not grow vs C mimalloc or jemalloc. `./tests/compiler-preload.sh` is the Rust-only slice. Jemalloc skips GCC/Clang C torture unless `JEMALLOC_FULL=1`. The oracle also links a `#[global_allocator]` stress binary with GNU ld (bfd), gold, LLVM LLD, mold, and Wild, and compiles rustc/gcc under `LD_PRELOAD` of each allocator for those linkers (`./tests/linkers.sh` / `cargo run -p mimalloc-harness -- linkers`). Nixpkgs mold is dynamically linked to C `libmimalloc-secure`; the flake overlay rebuilds mold with this rewrite **statically** linked (`nix build .#mold`). That mold has no `DT_NEEDED` mimalloc; `mi_malloc` is in the binary. `LD_PRELOAD` of another mimalloc onto it is skipped (two copies of the allocator in one process).

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

## Vulkan Memory Allocator

Pure-Rust GPU heap manager with AMD [VulkanMemoryAllocator](https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator) **3.4** C ABI (`vma*` / `Vma*` as in `vk_mem_alloc.h` v3.4.0). Include AMD's header without `VMA_IMPLEMENTATION` (or `crates/vma-c/include/vk_mem_alloc.h`) and link `libVulkanMemoryAllocator.so.3`. Vulkan is called only through `VmaVulkanFunctions` or `libvulkan.so.1`. The virtual allocator needs no GPU.

3.4 ABI extras vs 3.3: `VmaAllocationCreateInfo::minAlignment` (power of two, or 0); `VmaVulkanFunctions::vkGetPhysicalDeviceProperties2KHR`; `vmaAllocateDedicatedMemory` / `vmaCreateDedicatedBuffer` / `vmaCreateDedicatedImage` (optional `pMemoryAllocateNext` on `VkMemoryAllocateInfo`); `vmaGetMemoryWin32Handle2` (Linux stub returns `VK_ERROR_FEATURE_NOT_PRESENT`). `vmaCreateBufferWithAlignment` remains but is obsolete - it folds into `minAlignment`. `VMA_VERSION` is `VK_MAKE_VERSION(3, 4, 0)`. SONAME stays `.so.3`.

Tests: `vma-core` unit tests use in-process fake Vulkan, including a Blender GHOST / OpenXR-style workload (Vulkan 1.2, buffer device address, mapped sequential-write vertex/index/uniform buffers, GPU-only images, pools, stats/budgets/defrag, dedicated + `minAlignment`). The harness also compiles C smokes (`vma-virtual.c`, `vma-abi-34.c`, `vma-blender.c`) against the cdylib.

```
cd rust
cargo test -p vma-core
cargo build --release -p vma-c
./tests/vma-abi.sh
# or: cargo run -p mimalloc-harness -- vma
nix build .#vma
```

## NixOS

The flake overlay replaces `pkgs.mimalloc` with this library. Mitigations are always on; `mimalloc.override { secureBuild = true; }` is accepted so a live NixOS overlay that used C mimalloc's flag keeps evaluating.

```nix
{
  # `path:` copies gitignored rust/target (~4GiB) into the store; use git+file or github.
  inputs.mimalloc-rs.url = "git+file:///home/luluco/code/mimalloc";
  # and in nixos configuration:
  nixpkgs.overlays = [ mimalloc-rs.overlays.default ];
  environment.memoryAllocator.provider = "mimalloc";
  # or: imports = [ mimalloc-rs.nixosModules.memoryAllocator ];
}
```

`environment.memoryAllocator.provider = "mimalloc"` writes `${pkgs.mimalloc}/lib/libmimalloc.so` into `/etc/ld-nix.so.preload` (not `LD_PRELOAD`). On this rewrite that path is the Rust `cdylib`.

### World packages (build + run)

Packages from this machine (`NIXOS_CONFIG`, default `/etc/nixos`) are **run** as real workloads under the rewrite, C mimalloc, and libc - git, openssl, python3 (stdlib slice or store python), Node.js, gcc/g++/rustc/mold, KWin `--virtual --exit-with-session`, plasmashell, kreadconfig6, compression, e2fsprogs, … - not `--version`. PASS means stdout, stderr, and exit match libc. Rewrite-only mismatches are FAIL. Injection matches NixOS (`ld-nix.so.preload` via bubblewrap) and lists both `libmimalloc.so` and `libmimalloc-secure.so.3` so nixpkgs mold's `DT_NEEDED` binds the rewrite instead of C mimalloc. The flake check is the sandboxed subset (including python3, node, and mold).

```
nix build .#world-preload
nix build .#checks.x86_64-linux.world-preload
# PATH programs from this NixOS config:
cd rust && cargo run -p mimalloc-harness -- world
./tests/nixos-world.sh
```

A NixOS VM boots with the same `memoryAllocator` option (glibc reads `/etc/ld-nix.so.preload`):

```
nix build .#nixos-malloc
```

Firefox / Chromium / Electron are **run** as allocator smokes (startup, child `/proc/*/maps`, short headless page). Compiler-suite `LD_PRELOAD` is not a substitute. Hide-system-malloc PATH wraps are unwrapped; injection matches NixOS (`ld-nix.so.preload` via bubblewrap).

```
cd rust
# set ELECTRON / CHROMIUM if those binaries are not on PATH
./tests/browsers.sh
# or: cargo run -p mimalloc-harness -- browsers
# optional nixpkgs browsers (large): NIX_BROWSERS=1 ./tests/browsers.sh
nix build .#browsers-preload
```

A libc control must start, spawn children, and finish the page smoke. The rewrite is compared to C mimalloc (`MI_SECURE=FULL`) on the same recipe: a rewrite-only crash is a FAIL. C mimalloc skips `strdup`/`reallocarray`/`__libc_*` overrides by default (`-DMI_OVERRIDE_LIBC_EXTRAS=OFF`) so PartitionAlloc is not handed foreign pointers; both allocators should complete the smokes. Live-host allocator-exclusion wraps stay required when the OS preloads an older C mimalloc that still exports those aliases.

### Bun and Serde test suites

[Bun](https://github.com/oven-sh/bun/) (`test/js/web`, `test/js/node`, `test/js/bun` slices from the matching `bun-v*` tag) and [Serde](https://github.com/serde-rs/serde) (`cargo test --workspace --all-targets`) are **run** under the rewrite, C mimalloc, and libc. PASS means bun pass/fail/ran counts match libc, and serde stdout/stderr/exit match after stripping durations. Rewrite-only mismatches are FAIL. Cases that crash on system malloc are skipped. Injection matches NixOS (`ld-nix.so.preload` via bubblewrap) and lists both mimalloc SONAMEs.

```
cd rust
# nixpkgs bun is used if `bun` is not on PATH
# bun writes scratch under `/tmp/mimalloc-projects` (not `rust/target`) so a
# dirty Nix `path:` flake does not NAR-hash FIFOs / long paths
./tests/projects.sh
# or: cargo run -p mimalloc-harness -- projects
# PROJECTS=bun|serde|all  BUN_FULL=1  BUN_TEST='test/js/web/encoding'  BUN_SRC=  SERDE_SRC=
```

### Live system

**Session (does not switch the OS).** This host already preloads C mimalloc via `/etc/ld-nix.so.preload`. Stacking that with another `LD_PRELOAD` is two allocators. `nix run .#live` hides the preload in a mount namespace (bubblewrap) and then preloads the rewrite:

```
nix run .#live -- git --version
nix run .#live -- python3 -c 'print(sum(range(10000)))'
nix run .#live          # shell with the rewrite
```

**Whole OS.** Import the overlay (after or instead of `prev.mimalloc.override { secureBuild = true; }`) and keep `environment.memoryAllocator.provider = "mimalloc"`, then `nixos-rebuild test` (or `boot` / `switch`). Mozilla/Chromium/Electron wraps that bind-mount an empty preload file stay valid.

## Secure mitigations

Always on (inspired by C `-DMI_SECURE=FULL`): encoded free lists, padding canaries, slack-byte overflow checks (`0xDE`), double-free detection, randomized page free lists, guard pages around page metadata **and at the end of every mimalloc page**, and ASLR-style gaps between OS mappings. Encoded free-list next pointers use `ptr::addr` / `with_exposed_provenance_mut` rather than `as usize` / `as *mut`. Padding fill/compare uses SSE2 (x86_64) or NEON (aarch64). Lengths ≥ 64 bytes use AVX-512 (`AVX512F`+`AVX512BW`) when the CPU and OS enable ZMM state - including Zen 5 - otherwise SSE2. Sampled object guard pages are off until `mi_theap_guarded_set_sample_rate`. Install both `libmimalloc.so.3` and `libmimalloc-secure.so.3` so programs that `DT_NEEDED` the secure SONAME (for example nixpkgs mold) can `LD_PRELOAD` or replace the C library.

Release builds can enable C-style debug fill (`0xD0` / `0xDF`) with `--features debug-fill` (off by default; it is always on in the debug profile).

## LD_PRELOAD compiler stress

`tests/compiler-preload.sh` (Rust crate `mimalloc-harness`) compiles GCC, Clang, and rustc suite programs with the system toolchain, then runs the **same binaries** with `LD_PRELOAD`. PASS means stdout, stderr, and exit status match the system malloc - compile success is not enough. Cases that already fail with the system malloc are skipped so only allocator regressions count. `tests/oracle-suites.sh` repeats the runs under C mimalloc (`MI_SECURE=FULL`) and stock jemalloc (same binaries) and requires the Rust FAIL set to be a subset of both. Harness filters and output comparison are unit-tested (`cargo test -p mimalloc-harness`).

## Formal verification (Kani)

`mimalloc-core` has `#[cfg(kani)]` proofs for `align_up`, size-class `bin_for_size`, padding size, and free-list `encode_addr`/`decode_addr` (integer roundtrip). Host `cargo test -p mimalloc-core` covers the same properties with fixed inputs. Kani is not in nixpkgs; install it yourself, then:

```
cargo install --locked kani-verifier
cargo kani setup
./tests/kani.sh
# or: cargo kani -p mimalloc-core
```

Proofs stay on pure integer helpers (`addr` / `with_exposed_provenance_mut` for encoded free-list next pointers). SIMD fill/compare/copy (`core::arch` SSE2/NEON/AVX-512) is `cfg(not(kani))` so the verifier does not have to model vector instructions.

## Benchmarks

Same C binary under `LD_PRELOAD` of each malloc (glibc, Rust `libmimalloc.so`, Rust `libmimalloc-secure.so`, C mimalloc `MI_SECURE=FULL`, jemalloc), plus `#[global_allocator]` via `mimalloc-bench`. Reports wall time (`CLOCK_MONOTONIC`) and user-mode instruction counts (`perf_event_open` `PERF_COUNT_HW_INSTRUCTIONS`; 0 if the kernel denies counters). `BENCH_N` divides iteration counts (default `1`).

```
cd rust
./tests/bench.sh
# or: cargo run -p mimalloc-harness -- bench
cargo run --release -p mimalloc-bench
nix build .#mimalloc   # installs $out/bin/mimalloc-bench
```

`nix develop` provides `hyperfine` and `perf`. Set `HYPERFINE=1` to also run hyperfine on the C bench binary for each allocator.
