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
      overlays.default = final: prev: {
        mimalloc = final.callPackage ./rust/package.nix { };
        # Rebuild mold with the rewrite statically linked (nixpkgs mold
        # otherwise DT_NEEDEDs C libmimalloc-secure).
        mold-unwrapped = final.callPackage ./rust/mold.nix {
          inherit (prev) mold-unwrapped;
          # Don't re-run the mimalloc C ABI suite just to link mold.
          mimalloc = final.mimalloc.overrideAttrs (_: {
            doCheck = false;
          });
        };
      };

      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          mimallocUnchecked = pkgs.mimalloc.overrideAttrs (_: {
            doCheck = false;
          });
        in
        {
          default = pkgs.mimalloc;
          mimalloc = pkgs.mimalloc;
          mold = pkgs.mold;
          mold-unwrapped = pkgs.mold-unwrapped;
          mimalloc-musl = pkgs.callPackage ./rust/package.nix {
            rustPlatform = muslRustPlatform pkgs;
            cargoTarget = muslTargetFor pkgs;
            targetCc = pkgs.pkgsMusl.stdenv.cc;
          };
          world-preload = pkgs.callPackage ./rust/world.nix { mimalloc = mimallocUnchecked; };
          live = pkgs.callPackage ./rust/live.nix { mimalloc = mimallocUnchecked; };
          vma = pkgs.callPackage ./rust/vma.nix { };
          nixos-malloc = pkgs.testers.runNixOSTest {
            name = "mimalloc-memory-allocator";
            nodes.machine =
              { pkgs, ... }:
              {
                nixpkgs.overlays = [
                  (final: prev: {
                    mimalloc = prev.mimalloc.overrideAttrs (_: {
                      doCheck = false;
                    });
                  })
                ];
                environment.memoryAllocator.provider = "mimalloc";
                environment.systemPackages = [
                  pkgs.hello
                  pkgs.python3
                  pkgs.git
                  pkgs.gcc
                ];
              };
            testScript = ''
              machine.wait_for_unit("multi-user.target")
              machine.succeed("grep -q libmimalloc /etc/ld-nix.so.preload")
              machine.succeed("hello")
              machine.succeed("git --version")
              machine.succeed("python3 -c 'print(sum(range(10000)))'")
              machine.succeed(
                  "echo 'int main(void){return 0;}' > /tmp/t.c && gcc /tmp/t.c -o /tmp/t && /tmp/t"
              )
            '';
          };
        }
      );

      checks = forAllSystems (system: {
        # `buildRustPackage` runs cargo tests + C ABI / LD_PRELOAD checks.
        glibc = self.packages.${system}.mimalloc;
        musl = self.packages.${system}.mimalloc-musl;
        mold =
          let
            pkgs = pkgsFor system;
            moldBin = pkgs.mold-unwrapped;
          in
          pkgs.runCommand "mold-mimalloc-static" {
            nativeBuildInputs = [
              pkgs.gcc
              pkgs.binutils
              moldBin
            ];
          } ''
            mold --version
            if readelf -d ${moldBin}/bin/mold | grep NEEDED | grep -q libmimalloc; then
              echo "mold still DT_NEEDED libmimalloc" >&2
              exit 1
            fi
            nm ${moldBin}/bin/mold | grep -q ' mi_malloc'
            echo 'int main(void) { return 0; }' > t.c
            gcc -fuse-ld=mold t.c -o t
            ./t
            mkdir -p $out
            echo ok > $out/ok
          '';
        world-preload = self.packages.${system}.world-preload;
        vma = self.packages.${system}.vma;
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
              pkgs.lld
              pkgs.mold
              pkgs.wild
              pkgs.git
              pkgs.cmake
              pkgs.python3
              pkgs.wasmtime
              pkgs.gdb
              pkgs.jemalloc
              pkgs.pkgsMusl.stdenv.cc
              pkgs.hyperfine
              pkgs.perf
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

      # Overlay plus `environment.memoryAllocator.provider = "mimalloc"`.
      # On a host that already uses C mimalloc, this replaces `pkgs.mimalloc`
      # so /etc/ld-nix.so.preload points at the rewrite (always-on secure).
      nixosModules.memoryAllocator =
        { lib, ... }:
        {
          imports = [ self.nixosModules.default ];
          environment.memoryAllocator.provider = lib.mkDefault "mimalloc";
        };
    };
}
