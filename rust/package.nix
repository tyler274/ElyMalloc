{
  lib,
  rustPlatform,
  stdenv,
  binutils,
  # NixOS `mimalloc.override { secureBuild = true; }` (C package). Mitigations
  # are always on here; the flag is accepted so the live overlay keeps working.
  secureBuild ? true,
  # When set (musl check/package), compile with this rustc target and musl cc
  # rather than rebuilding rustc against musl.
  cargoTarget ? null,
  targetCc ? stdenv.cc,
}:

let
  isMusl = cargoTarget != null || stdenv.hostPlatform.isMusl;
  target = if cargoTarget != null then cargoTarget else stdenv.hostPlatform.rust.rustcTarget;
  targetFlag = lib.optionalString (cargoTarget != null) "--target ${cargoTarget}";
  cargoEnvTarget = lib.toUpper (builtins.replaceStrings [ "-" ] [ "_" ] target);
  ccBin = "${targetCc}/bin/${targetCc.targetPrefix}cc";
  cxxBin = "${targetCc}/bin/${targetCc.targetPrefix}c++";
in
rustPlatform.buildRustPackage {
  pname = "mimalloc";
  version = "3.5.0";

  src = ./.;

  cargoLock.lockFile = ./Cargo.lock;

  # cargo-auditable would pull a second rustc (and LLVM) on musl.
  auditable = false;

  # rustPlatform's cargoBuildHook is baked to the host gnu target; drive
  # musl ourselves so we do not rebuild rustc.
  preBuild = lib.optionalString (cargoTarget != null) ''
    export CARGO_BUILD_TARGET=${cargoTarget}
  '';
  buildPhase = ''
    runHook preBuild
    cargo build --offline --release ${targetFlag} -p mimalloc-c
    cargo build --offline --release ${targetFlag} -p mimalloc-c --features secure --target-dir target/mimalloc-secure
    cargo build --offline --release ${targetFlag} -p mimalloc-bench
    runHook postBuild
  '';

  # Musl defaults to fully static binaries, which drop `cdylib`. NixOS
  # `memoryAllocator` and LD_PRELOAD need the shared object, so keep it.
  env = {
    RUSTFLAGS = lib.optionalString isMusl "-C target-feature=-crt-static";
  }
  // lib.optionalAttrs isMusl {
    "CARGO_TARGET_${cargoEnvTarget}_LINKER" = ccBin;
    CARGO_BUILD_TARGET = target;
  };

  nativeCheckInputs = [
    targetCc
    binutils
  ];

  doCheck = true;
  checkPhase = ''
    runHook preCheck
    cargo test --offline -p mimalloc-core --release ${targetFlag}
    cargo test --offline -p mimalloc-wasm-smoke --release ${targetFlag}
    cargo test --offline -p mimalloc-harness --release ${targetFlag}
    cargo test --offline -p mimalloc-alloc-stress --release ${targetFlag}
    ${lib.optionalString (cargoTarget == null) ''
    cargo check --offline -p mimalloc-core --target wasm32-unknown-unknown
    cargo check --offline -p mimalloc-wasm-smoke --target wasm32-unknown-unknown
    ''}
    cargo build --release -p mimalloc-c ${targetFlag}
    cargo build --release -p mimalloc-c --features secure --target-dir target/mimalloc-secure ${targetFlag}

    so="target/${target}/release/libmimalloc.so"
    if [ ! -f "$so" ]; then
      so="target/release/libmimalloc.so"
    fi
    if [ ! -f "$so" ]; then
      echo "libmimalloc.so was not produced (musl needs -C target-feature=-crt-static)" >&2
      exit 1
    fi

    cargo build -p mimalloc-c ${targetFlag}
    debug_so="target/${target}/debug/libmimalloc.so"
    if [ ! -f "$debug_so" ]; then
      debug_so="target/debug/libmimalloc.so"
    fi
    export CC=${lib.escapeShellArg ccBin}
    export CXX=${lib.escapeShellArg cxxBin}
    export SO="$(pwd)/$so"
    export DEBUG_SO="$(pwd)/$debug_so"
    export INCLUDE=${../include}
    export C_TESTS=${./tests}
    export UPSTREAM_TESTS=${../test}
    export OUT="$TMPDIR/mi-c-abi"
    cargo run --offline --release ${targetFlag} -p mimalloc-harness -- c-abi
    secure_so="target/mimalloc-secure/${target}/release/libmimalloc.so"
    if [ ! -f "$secure_so" ]; then
      secure_so="target/mimalloc-secure/release/libmimalloc.so"
    fi
    if [ ! -f "$secure_so" ]; then
      echo "libmimalloc-secure.so was not produced" >&2
      exit 1
    fi
    mkdir -p "$TMPDIR/mi-secure-so"
    cp "$secure_so" "$TMPDIR/mi-secure-so/libmimalloc-secure.so"
    unset DEBUG_SO
    export SO="$TMPDIR/mi-secure-so/libmimalloc-secure.so"
    export OUT="$TMPDIR/mi-c-abi-secure"
    cargo run --offline --release ${targetFlag} -p mimalloc-harness -- c-abi
    runHook postCheck
  '';

  dontCargoInstall = true;

  postBuild = ''
    so="target/${target}/release/libmimalloc.so"
    archive="target/${target}/release/libmimalloc.a"
    if [ ! -f "$so" ]; then
      so="target/release/libmimalloc.so"
      archive="target/release/libmimalloc.a"
    fi
    if [ ! -f "$so" ]; then
      echo "libmimalloc.so was not produced (musl needs -C target-feature=-crt-static)" >&2
      exit 1
    fi
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib $out/include

    so="target/${target}/release/libmimalloc.so"
    archive="target/${target}/release/libmimalloc.a"
    if [ ! -f "$so" ]; then
      so="target/release/libmimalloc.so"
      archive="target/release/libmimalloc.a"
    fi

    if [ ! -f "$so" ]; then
      echo "libmimalloc.so was not produced" >&2
      exit 1
    fi
    cp "$so" $out/lib/libmimalloc.so.3
    ln -s libmimalloc.so.3 $out/lib/libmimalloc.so
    if [ -f "$archive" ]; then
      cp "$archive" $out/lib/libmimalloc.a
    fi

    secure_so="target/mimalloc-secure/${target}/release/libmimalloc.so"
    if [ ! -f "$secure_so" ]; then
      secure_so="target/mimalloc-secure/release/libmimalloc.so"
    fi
    if [ ! -f "$secure_so" ]; then
      echo "libmimalloc-secure.so was not produced" >&2
      exit 1
    fi
    cp "$secure_so" $out/lib/libmimalloc-secure.so.3
    ln -s libmimalloc-secure.so.3 $out/lib/libmimalloc-secure.so
    if [ -f "target/mimalloc-secure/release/libmimalloc.a" ]; then
      cp target/mimalloc-secure/release/libmimalloc.a $out/lib/libmimalloc-secure.a
    elif [ -f "target/mimalloc-secure/${target}/release/libmimalloc.a" ]; then
      cp "target/mimalloc-secure/${target}/release/libmimalloc.a" $out/lib/libmimalloc-secure.a
    fi

    cp ${../include}/mimalloc.h $out/include/
    cp ${../include}/mimalloc-stats.h $out/include/
    cp ${../include}/mimalloc-override.h $out/include/
    cp ${../include}/mimalloc-new-delete.h $out/include/
    cp -R ${../include}/mimalloc $out/include/

    mkdir -p $out/lib/cmake/mimalloc $out/lib/pkgconfig
    cp ${./cmake/mimalloc-config.cmake} $out/lib/cmake/mimalloc/mimalloc-config.cmake
    cp ${./cmake/mimalloc-config-version.cmake} $out/lib/cmake/mimalloc/mimalloc-config-version.cmake

    bench="target/${target}/release/mimalloc-bench"
    if [ ! -f "$bench" ]; then
      bench="target/release/mimalloc-bench"
    fi
    if [ -f "$bench" ]; then
      mkdir -p $out/bin
      cp "$bench" $out/bin/mimalloc-bench
    fi

    cat > $out/lib/pkgconfig/mimalloc.pc <<EOF
prefix=$out
libdir=''${prefix}/lib
includedir=''${prefix}/include

Name: mimalloc
Description: Pure-Rust mimalloc rewrite
Version: 3.5.0
Libs: -L''${libdir} -lmimalloc-secure
Cflags: -I''${includedir}
EOF
    runHook postInstall
  '';

  meta = with lib; {
    description = "Pure-Rust rewrite of mimalloc with a C ABI drop-in";
    homepage = "https://github.com/microsoft/mimalloc";
    license = licenses.mit;
    platforms = platforms.linux;
  };
}
