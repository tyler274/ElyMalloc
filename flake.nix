{
  description = "Pure-Rust mimalloc rewrite with a C ABI drop-in";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      overlays.default = final: _prev: {
        mimalloc = final.callPackage ./rust/package.nix { };
      };

      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ self.overlays.default ];
          };
        in
        {
          default = pkgs.mimalloc;
          mimalloc = pkgs.mimalloc;
        }
      );

      nixosModules.default =
        { ... }:
        {
          nixpkgs.overlays = [ self.overlays.default ];
        };
    };
}
