#!/usr/bin/env bash
# Headless replay of GitHub Rewrite *linux* jobs in Docker before push.
#
# Darwin is a separate VM: ./tests/ci-docker-osx.sh (sickcodes/Docker-OSX,
# Intel macOS over KVM). That is not macos-14 arm64, but it does run malloc
# zones, host EAGAIN, and PROT_NONE SIGBUS. See rust/README.md.
#
#   ./tests/ci-docker.sh           # linux-x64 job
#   CROSS=1 ./tests/ci-docker.sh   # also aarch64 + riscv64 qemu smokes
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
unset LD_PRELOAD || true

IMAGE="${REWRITE_CI_IMAGE:-mimalloc-rewrite-ci}"
docker build -t "$IMAGE" "$ROOT/contrib/docker/rewrite-ci"

linux_x64() {
  local cargo_home="${CARGO_HOME:-$HOME/.cargo}"
  local vols=(
    -v "$ROOT:/src:ro"
    -w /src/rust
  )
  if [[ -d "$cargo_home/registry" ]]; then
    vols+=(-v "$cargo_home/registry:/usr/local/cargo/registry")
  fi
  if [[ -d "$cargo_home/git" ]]; then
    vols+=(-v "$cargo_home/git:/usr/local/cargo/git")
  fi
  docker run --rm --network host \
    -e CARGO_TERM_COLOR=always \
    -e CARGO_TARGET_DIR=/tmp/target \
    "${vols[@]}" \
    "$IMAGE" \
    bash -c 'cargo test -p mimalloc-core --release --offline && cargo build --release --offline -p mimalloc-c && cargo run --offline -p mimalloc-harness -- c-abi'
}

cross_qemu() {
  local cargo_home="${CARGO_HOME:-$HOME/.cargo}"
  local vols=(
    -v "$ROOT:/src:ro"
    -w /src/rust
  )
  if [[ -d "$cargo_home/registry" ]]; then
    vols+=(-v "$cargo_home/registry:/usr/local/cargo/registry")
  fi
  docker run --rm --network host \
    -e CARGO_TERM_COLOR=always \
    -e CARGO_TARGET_DIR=/tmp/target \
    -e MIMALLOC_QEMU=1 \
    -e RUST_TEST_THREADS=1 \
    "${vols[@]}" \
    "$IMAGE" \
    bash -c '
      apt-get update && apt-get install -y --no-install-recommends \
        qemu-user gcc-aarch64-linux-gnu gcc-riscv64-linux-gnu
      rustup target add aarch64-unknown-linux-gnu riscv64gc-unknown-linux-gnu
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER="qemu-aarch64 -L /usr/aarch64-linux-gnu"
      export CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER=riscv64-linux-gnu-gcc
      export CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_RUNNER="qemu-riscv64 -L /usr/riscv64-linux-gnu"
      cargo test --offline -p mimalloc-core --release --target aarch64-unknown-linux-gnu -- --test-threads=1
      cargo build --offline --release -p mimalloc-c --target aarch64-unknown-linux-gnu
      aarch64-linux-gnu-gcc -O2 -pthread tests/smoke.c -o /tmp/mi-smoke-aarch64
      qemu-aarch64 -L /usr/aarch64-linux-gnu \
        -E LD_PRELOAD="$CARGO_TARGET_DIR/aarch64-unknown-linux-gnu/release/libmimalloc.so" \
        /tmp/mi-smoke-aarch64
      cargo test --offline -p mimalloc-core --release --target riscv64gc-unknown-linux-gnu -- --test-threads=1
      cargo build --offline --release -p mimalloc-c --target riscv64gc-unknown-linux-gnu
      riscv64-linux-gnu-gcc -O2 -pthread tests/smoke.c -o /tmp/mi-smoke-riscv
      qemu-riscv64 -L /usr/riscv64-linux-gnu \
        -E LD_PRELOAD="$CARGO_TARGET_DIR/riscv64gc-unknown-linux-gnu/release/libmimalloc.so" \
        /tmp/mi-smoke-riscv
    '
}

linux_x64
if [[ "${CROSS:-0}" == "1" ]]; then
  cross_qemu
fi
echo "ci-docker: linux rewrite jobs passed"
