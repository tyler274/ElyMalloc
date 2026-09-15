# Use ElyMalloc as a first-class NixOS malloc provider before the
# `elymalloc` enum value lands in nixpkgs. Replaces `config/malloc.nix`
# with this tree's copy (same file as the nixpkgs PR).
#
# Does **not** set `nixpkgs.overlays`: `runNixOSTest` treats that option as
# read-only. Apply `overlays.default` from the host flake (or import
# `nixosModules.memoryAllocator`, which wraps this module plus the overlay).
{ lib, ... }:
{
  disabledModules = [ "config/malloc.nix" ];
  imports = [ ./config/malloc.nix ];
  environment.memoryAllocator.provider = lib.mkDefault "elymalloc";
}
