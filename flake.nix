{
  description = "ElyMalloc: pure-Rust mimalloc rewrite with a C ABI drop-in";

  # Consumers: use `git+file:///abs/path` or github:, not `path:/…`. A path
  # input copies gitignored `rust/target` into the Nix store (~4GiB/update).

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
        kani = final.callPackage ./rust/kani.nix { };
        # First-class package for `environment.memoryAllocator.provider =
        # "elymalloc"`. Does **not** replace `pkgs.mimalloc` (C mimalloc).
        elymalloc = final.callPackage ./rust/package.nix { };
        # Rebuild mold with ElyMalloc statically linked (nixpkgs mold
        # otherwise DT_NEEDs C libmimalloc-secure).
        mold-unwrapped = final.callPackage ./rust/mold.nix {
          inherit (prev) mold-unwrapped;
          mimalloc = final.elymalloc.overrideAttrs (_: {
            doCheck = false;
          });
        };
      };

      # Migration hatch: `pkgs.mimalloc` becomes ElyMalloc so existing
      # `provider = "mimalloc"` configs keep the rewrite. Prefer
      # `provider = "elymalloc"` + `overlays.default` for nixpkgs.
      overlays.replaceMimalloc = final: prev: {
        mimalloc = final.elymalloc;
      };

      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          elymallocUnchecked = pkgs.elymalloc.overrideAttrs (_: {
            doCheck = false;
          });
        in
        {
          default = pkgs.elymalloc;
          elymalloc = pkgs.elymalloc;
          # Alias: `nix build .#mimalloc` still builds the rewrite.
          mimalloc = pkgs.elymalloc;
          kani = pkgs.kani;
          mold = pkgs.mold;
          mold-unwrapped = pkgs.mold-unwrapped;
          elymalloc-musl = pkgs.callPackage ./rust/package.nix {
            rustPlatform = muslRustPlatform pkgs;
            cargoTarget = muslTargetFor pkgs;
            targetCc = pkgs.pkgsMusl.stdenv.cc;
          };
          mimalloc-musl = self.packages.${system}.elymalloc-musl;
          world-preload = pkgs.callPackage ./rust/world.nix {
            mimalloc = elymallocUnchecked;
            # Vanilla nixpkgs mold DT_NEEDs C libmimalloc-secure; dual-soname
            # preload must make linking work without LD_LIBRARY_PATH.
            mold = nixpkgs.legacyPackages.${system}.mold;
            nodejs = pkgs.nodejs;
          };
          browsers-preload = pkgs.callPackage ./rust/browsers.nix { mimalloc = elymallocUnchecked; };
          live = pkgs.callPackage ./rust/live.nix { mimalloc = elymallocUnchecked; };
          vma = pkgs.callPackage ./rust/vma.nix { };
          nixos-malloc =
            let
              testPkgs = pkgs.extend (_: _: { elymalloc = elymallocUnchecked; });
            in
            testPkgs.testers.runNixOSTest {
              name = "elymalloc-memory-allocator";
              nodes.machine =
                { pkgs, ... }:
                {
                  imports = [ ./nixos/modules/elymalloc.nix ];
                  environment.systemPackages = [
                    pkgs.hello
                    pkgs.python3
                    pkgs.nodejs
                    pkgs.git
                    pkgs.gcc
                  ];
                };
              testScript = ''
                machine.wait_for_unit("multi-user.target")
                machine.succeed("grep -q malloc-provider-elymalloc /etc/ld-nix.so.preload")
                machine.succeed("grep -q libmimalloc.so /etc/ld-nix.so.preload")
                machine.succeed("grep -q libmimalloc-secure.so.3 /etc/ld-nix.so.preload")
                machine.succeed("hello")
                machine.succeed("git --version")
                machine.succeed("python3 -c 'print(sum(range(10000)))'")
                machine.succeed("node -e 'console.log(\"node-ok\", Buffer.alloc(64).length)'")
                machine.succeed(
                    "echo 'int main(void){return 0;}' > /tmp/t.c && gcc /tmp/t.c -o /tmp/t && /tmp/t"
                )
              '';
            };
        }
      );

      checks = forAllSystems (system: {
        # `buildRustPackage` runs cargo tests + C ABI / LD_PRELOAD checks.
        glibc = self.packages.${system}.elymalloc;
        musl = self.packages.${system}.elymalloc-musl;
        mold =
          let
            pkgs = pkgsFor system;
            moldBin = pkgs.mold-unwrapped;
          in
          pkgs.runCommand "mold-elymalloc-static" {
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
            readelf -s --wide ${moldBin}/bin/mold > mold.syms
            grep -F -w -- mi_malloc mold.syms >/dev/null
            echo 'int main(void) { return 0; }' > t.c
            gcc -fuse-ld=mold t.c -o t
            ./t
            mkdir -p $out
            echo ok > $out/ok
          '';
        world-preload = self.packages.${system}.world-preload;
        nixos-malloc = self.packages.${system}.nixos-malloc;
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
              pkgs.kani
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

      # Extends `environment.memoryAllocator.provider` with `"elymalloc"`.
      # Does not apply the overlay (needed by `runNixOSTest`).
      nixosModules.malloc = ./nixos/modules/elymalloc.nix;

      # Overlay + malloc module. `provider` defaults to `"elymalloc"`.
      nixosModules.memoryAllocator = {
        imports = [
          self.nixosModules.default
          self.nixosModules.malloc
        ];
      };
    };
}
