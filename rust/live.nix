# Session wrapper: run a command (or a shell) with this rewrite as malloc.
# On a live NixOS host that already sets environment.memoryAllocator, glibc
# also loads /etc/ld-nix.so.preload (often C mimalloc). Stacking both is
# unsafe, so this hides that preload in a mount namespace (same idea as the
# host's allocator-exclusions overlay) and then LD_PRELOADs the rewrite.
{
  mimalloc,
  bubblewrap,
  writeText,
  writeShellScriptBin,
  runtimeShell,
}:

let
  so = "${mimalloc}/lib/libmimalloc.so";
  empty = writeText "empty-ld-nix.so.preload" "";
in
writeShellScriptBin "mimalloc-live" ''
  #!${runtimeShell}
  set -euo pipefail
  so=${so}
  empty=${empty}
  bwrap=${bubblewrap}/bin/bwrap

  if [ ! -f "$so" ]; then
    echo "mimalloc-live: missing $so" >&2
    exit 1
  fi

  export LD_PRELOAD="$so''${LD_PRELOAD:+:$LD_PRELOAD}"

  if [ "$#" -eq 0 ]; then
    echo "mimalloc-live: LD_PRELOAD=$LD_PRELOAD" >&2
    echo "session wrapper; for the whole OS import nixosModules.memoryAllocator and nixos-rebuild" >&2
    set -- "''${SHELL:-${runtimeShell}}"
  fi

  add_bind() {
    local p=$1
    [ -n "$p" ] && [ -f "$p" ] && [ ! -L "$p" ] || return 0
    local b
    for b in "''${binds[@]+"''${binds[@]}"}"; do
      [ "$b" = "$p" ] && return 0
    done
    binds+=("$p")
  }

  binds=()
  if [ -e /etc/ld-nix.so.preload ] || [ -L /etc/ld-nix.so.preload ]; then
    add_bind /etc/ld-nix.so.preload
    add_bind "$(readlink -f /etc/ld-nix.so.preload 2>/dev/null || true)"
    add_bind /etc/static/ld-nix.so.preload
    add_bind "$(readlink -f /etc/static/ld-nix.so.preload 2>/dev/null || true)"
  fi

  if [ "''${#binds[@]}" -eq 0 ]; then
    exec "$@"
  fi

  args=(--bind / / --dev-bind /dev /dev --proc /proc --die-with-parent)
  for f in "''${binds[@]}"; do
    args+=(--ro-bind "$empty" "$f")
  done
  exec "$bwrap" "''${args[@]}" "$@"
''
