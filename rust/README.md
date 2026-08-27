# Rust mimalloc rewrite (Phase 1)

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
