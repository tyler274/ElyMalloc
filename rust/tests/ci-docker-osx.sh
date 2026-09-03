#!/usr/bin/env bash
# Headless Darwin via https://github.com/sickcodes/Docker-OSX (OSX-KVM in Docker).
#
# This is Intel macOS (x86_64 QEMU/KVM), not GitHub's macos-14 arm64. It still
# catches the failures we have been chasing: Darwin malloc-zone fork, host
# EAGAIN (35), and PROT_NONE -> SIGBUS. It will not catch 16 KiB pages / 128 KiB
# slices (Apple silicon only).
#
# First run pulls the auto image (~tens of GB) and boots several minutes (the
# image copies the disk between layers before sshd answers). Username `user`,
# password `alpine`. Needs /dev/kvm.
#
# Hub no longer publishes sickcodes/docker-osx:auto (manifest unknown). The
# Docker-OSX issue tracker points at dickhub/docker-osx as the remaining
# :auto / :naked tags. Override with OSX_AUTO_IMAGE if that changes.
#
# The :auto image's PID 1 launches qemu, waits for guest SSH, then runs
# OSX_COMMANDS over that session. When that command returns, qemu is killed.
# Guest macOS `sleep` does not accept `infinity`, so the keepalive is a loop.
#
#   ./tests/ci-docker-osx.sh              # start VM if needed, SSH, run rewrite macos job
#   STOP=1 ./tests/ci-docker-osx.sh       # stop the VM after tests
#   PULL=1 ./tests/ci-docker-osx.sh       # docker pull before start
#   FORCE=1 ./tests/ci-docker-osx.sh      # remove a stale container and recreate
#
# Optional: OSX_IMAGE=/path/to/mac_hdd_ng_auto.img uses the naked image
# instead of :auto. OSX_PORT (default 50922), OSX_RAM (default 8).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
unset LD_PRELOAD || true

NAME="${OSX_NAME:-mimalloc-docker-osx}"
PORT="${OSX_PORT:-50922}"
RAM="${OSX_RAM:-8}"
USER_NAME="${OSX_USER:-user}"
PASS="${OSX_PASS:-alpine}"
AUTO_IMAGE="${OSX_AUTO_IMAGE:-dickhub/docker-osx:auto}"
NAKED_IMAGE="${OSX_NAKED_IMAGE:-dickhub/docker-osx:naked}"
# Darwin /bin/sleep only takes a number. Keep the auto-image SSH session open
# so PID 1 does not exit and take qemu with it.
KEEPALIVE='while true; do sleep 86400; done'

if [[ ! -e /dev/kvm ]]; then
  echo "ci-docker-osx: /dev/kvm missing (enable KVM)" >&2
  exit 1
fi

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "ci-docker-osx: need $1 on PATH" >&2
    exit 1
  }
}
need docker
need ssh
need rsync
if ! command -v sshpass >/dev/null 2>&1; then
  if command -v nix-shell >/dev/null 2>&1; then
    exec nix-shell -p sshpass --run "$(printf '%q ' "$0" "$@")"
  fi
  echo "ci-docker-osx: need sshpass (nix-shell -p sshpass)" >&2
  exit 1
fi

SSH_OPTS=(
  -p "$PORT"
  -o StrictHostKeyChecking=no
  -o UserKnownHostsFile=/dev/null
  -o PreferredAuthentications=password
  -o PubkeyAuthentication=no
  -o LogLevel=ERROR
  -o ConnectTimeout=5
  -o ConnectionAttempts=1
  -o ServerAliveInterval=30
  -o ServerAliveCountMax=120
)

osx_ssh() {
  sshpass -p "$PASS" ssh "${SSH_OPTS[@]}" "${USER_NAME}@127.0.0.1" "$@"
}

# Quoted-heredoc guest script. Do not use an unquoted <<EOF here: host bash
# would expand the body (and `cargo \build` / PATH `\$HOME` quoting has already
# shown up as `uild: command not found` on the host).
osx_bash() {
  sshpass -p "$PASS" ssh "${SSH_OPTS[@]}" "${USER_NAME}@127.0.0.1" \
    "export RUSTUP_TOOLCHAIN=$(printf '%q' "$RUSTUP_TOOLCHAIN"); export SUDO_PASS=$(printf '%q' "$PASS"); exec bash -s"
}

osx_up() {
  docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null | grep -q true
}

start_vm() {
  if [[ "${FORCE:-0}" == "1" ]]; then
    docker rm -f "$NAME" >/dev/null 2>&1 || true
  fi
  if osx_up; then
    echo "ci-docker-osx: container $NAME already running"
    return
  fi
  if docker inspect "$NAME" >/dev/null 2>&1; then
    # Stale containers were often created with `sleep infinity`, which Darwin
    # rejects; recreate so OSX_COMMANDS is a numeric sleep loop.
    echo "ci-docker-osx: replacing stopped container $NAME"
    docker rm -f "$NAME" >/dev/null
  fi
  if [[ "${PULL:-0}" == "1" ]]; then
    if [[ -n "${OSX_IMAGE:-}" ]]; then
      docker pull "$NAKED_IMAGE"
    else
      docker pull "$AUTO_IMAGE"
    fi
  fi
  local extra=(
    --name "$NAME"
    --device /dev/kvm
    --dns 8.8.8.8
    --dns 1.1.1.1
    -p "127.0.0.1:${PORT}:10022"
    -e "RAM=${RAM}"
    -e SMP=4
    -e CORES=4
    -e NOPICKER=true
    -e GENERATE_UNIQUE=true
    -e TERMS_OF_USE=i_agree
    -e DISPLAY=:99
    -e HEADLESS=true
    -e "USERNAME=${USER_NAME}"
    -e "PASSWORD=${PASS}"
    -e "OSX_COMMANDS=${KEEPALIVE}"
  )
  if [[ -n "${OSX_IMAGE:-}" ]]; then
    extra+=(-v "${OSX_IMAGE}:/image" "$NAKED_IMAGE")
  else
    extra+=("$AUTO_IMAGE")
  fi
  echo "ci-docker-osx: docker run ${extra[*]}"
  docker run -d "${extra[@]}"
}

wait_ssh() {
  echo "ci-docker-osx: waiting for SSH on 127.0.0.1:${PORT} (disk copy + boot can take 15+ min)"
  local i
  for i in $(seq 1 360); do
    if osx_ssh 'true' >/dev/null 2>&1; then
      echo "ci-docker-osx: SSH up after ${i} attempts"
      return 0
    fi
    if (( i % 12 == 0 )); then
      echo "ci-docker-osx: still waiting (${i}/360)"
    fi
    sleep 5
  done
  echo "ci-docker-osx: SSH did not come up (docker logs $NAME)" >&2
  docker logs --tail 80 "$NAME" >&2 || true
  exit 1
}

bootstrap_guest() {
  osx_bash <<'EOF'
set -euo pipefail
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
if ! command -v rustc >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
# /usr/bin/cc exists as an xcode-select stub even without CLT. Probe the
# preprocessor; if that fails, install CLT via softwareupdate (headless).
cc_works() {
  cc -E -xc /dev/null >/dev/null 2>&1
}
if ! cc_works; then
  echo "ci-docker-osx: no working cc; installing Command Line Tools"
  label="$(softwareupdate -l 2>/dev/null | sed -n 's/^[[:space:]]*\* Label: \(Command Line Tools.*\)/\1/p' | tail -1)"
  if [[ -z "$label" ]]; then
    echo "ci-docker-osx: softwareupdate listed no Command Line Tools package" >&2
    exit 1
  fi
  printf '%s\n' "${SUDO_PASS:?}" | sudo -S softwareupdate -i "$label"
  if [[ -d /Library/Developer/CommandLineTools ]]; then
    printf '%s\n' "$SUDO_PASS" | sudo -S xcode-select --switch /Library/Developer/CommandLineTools
  fi
  if ! cc_works; then
    echo "ci-docker-osx: cc still missing after CLT install" >&2
    exit 1
  fi
fi
rustc --version
cc --version | head -1
uname -a
EOF
}

sync_tree() {
  local rsh
  rsh="$(mktemp)"
  trap 'rm -f "$rsh"' RETURN
  {
    printf '#!/bin/sh\nexec'
    printf ' %q' sshpass -p "$PASS" ssh "${SSH_OPTS[@]}"
    printf ' "$@"\n'
  } >"$rsh"
  chmod +x "$rsh"
  rsync -az --delete \
    -e "$rsh" \
    --exclude '.git/' \
    --exclude 'rust/target/' \
    --exclude 'out/' \
    "$ROOT/" "${USER_NAME}@127.0.0.1:mimalloc/"
}

run_macos_job() {
  osx_bash <<'EOF'
set -euo pipefail
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
if command -v brew >/dev/null 2>&1 && [[ -d "$(brew --prefix llvm 2>/dev/null)/bin" ]]; then
  export PATH="$(brew --prefix llvm)/bin:$PATH"
fi
cd "$HOME/mimalloc/rust"
cargo test -p mimalloc-core --release
cargo build --release -p mimalloc-c
cargo run -p mimalloc-harness -- c-abi
EOF
}

start_vm
wait_ssh
bootstrap_guest
sync_tree
run_macos_job

if [[ "${STOP:-0}" == "1" ]]; then
  docker stop "$NAME" >/dev/null
  echo "ci-docker-osx: stopped $NAME"
fi
echo "ci-docker-osx: Darwin rewrite job passed (x86_64 Docker-OSX, not macos-14 arm64)"
