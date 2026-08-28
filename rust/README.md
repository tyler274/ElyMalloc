# Rust mimalloc rewrite

Pure-Rust allocator with a C ABI intended as a drop-in replacement for C mimalloc.

## Build

```
cd rust
cargo build --release -p mimalloc-c
```

This produces `target/release/libmimalloc.so` with SONAME `libmimalloc.so.3`.

## Test

```
cd rust
./tests/run.sh
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

Always on (inspired by C `-DMI_SECURE=ON`): encoded free lists, padding canaries, double-free detection, randomized page free lists, guard pages around page metadata, and ASLR-style gaps between OS mappings. Sampled object guard pages are off until `mi_theap_guarded_set_sample_rate`.

## Later

- Stress-test this library as a drop-in malloc (`LD_PRELOAD` / NixOS `memoryAllocator`) against the **GCC, LLVM, and rustc test suites**.
- Debug fill in release builds.
- musl / aarch64.
