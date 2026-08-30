{
  description = "Pure-Rust mimalloc rewrite with a C ABI drop-in";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Host rustc from nixpkgs has no musl std; rust-overlay supplies rust-std
    # for musl without rebuilding rustc against musl.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [
            rust-overlay.overlays.default
            self.overlays.default
          ];
        };
      muslTargetFor = pkgs: pkgs.pkgsMusl.stdenv.hostPlatform.rust.rustcTarget;
      # Host rustc + rust-std-musl from rust-overlay. Do not use
      # pkgsMusl.makeRustPlatform: that rebuilds LLVM/rustc against musl.
      muslRustPlatform =
        pkgs:
        let
          rust = pkgs.rust-bin.stable.latest.minimal.override {
            targets = [ (muslTargetFor pkgs) ];
          };
        in
        pkgs.makeRustPlatform {
          rustc = rust;
          cargo = rust;
        };
    in
    {
      overlays.default = final: _prev: {
        mimalloc = final.callPackage ./rust/package.nix { };
      };

      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mimalloc;
          mimalloc = pkgs.mimalloc;
          mimalloc-musl = pkgs.callPackage ./rust/package.nix {
            rustPlatform = muslRustPlatform pkgs;
            cargoTarget = muslTargetFor pkgs;
            targetCc = pkgs.pkgsMusl.stdenv.cc;
          };
        }
      );

      checks = forAllSystems (system: {
        # `buildRustPackage` runs cargo tests + C ABI / LD_PRELOAD checks.
        glibc = self.packages.${system}.mimalloc;
        musl = self.packages.${system}.mimalloc-musl;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          muslTarget = pkgs.pkgsMusl.stdenv.hostPlatform.rust.rustcTarget;
          rust = pkgs.rust-bin.stable.latest.minimal.override {
            targets = [
              pkgs.stdenv.hostPlatform.rust.rustcTarget
              muslTarget
              "wasm32-unknown-unknown"
              "wasm32-wasip1"
            ];
          };
        in
        {
          default = pkgs.mkShell {
            packages = [
              rust
              pkgs.gcc
              pkgs.clang
              pkgs.binutils
              pkgs.git
              pkgs.cmake
              pkgs.python3
              pkgs.wasmtime
              pkgs.gdb
              pkgs.jemalloc
              pkgs.pkgsMusl.stdenv.cc
            ];
            shellHook = ''
              export JEMALLOC_SO="${pkgs.jemalloc}/lib/libjemalloc.so"
            '';
          };
        }
      );

      nixosModules.default =
        { ... }:
        {
          nixpkgs.overlays = [ self.overlays.default ];
        };
    };
}
