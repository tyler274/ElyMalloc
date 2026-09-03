# Replay GitHub Rewrite linux-x64 locally. gcc/g++ ship in rust:bookworm;
# skip apt so the image builds offline from a pulled base (Debian mirrors
# often time out from this host). CROSS=1 installs qemu/cross gcc at run time.
FROM rust:1-bookworm
WORKDIR /src/rust
ENV RUSTUP_TOOLCHAIN=stable
CMD ["bash", "-c", "cargo test -p mimalloc-core --release && cargo build --release -p mimalloc-c && cargo run -p mimalloc-harness -- c-abi"]
