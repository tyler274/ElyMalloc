{
  lib,
  rustPlatform,
  stdenv,
}:

rustPlatform.buildRustPackage rec {
  pname = "mimalloc";
  version = "3.5.0";

  src = ./.;

  cargoLock.lockFile = ./Cargo.lock;

  # Workspace tests live in mimalloc-core; the cdylib has no Rust tests.
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    cargo test --offline -p mimalloc-core --release
    runHook postCheck
  '';

  # cdylib/staticlib are not installed by the default cargo-install step.
  dontCargoInstall = true;

  postBuild = ''
    so="target/${stdenv.hostPlatform.rust.cargoShortTarget}/release/libmimalloc.so"
    if [ ! -f "$so" ]; then
      so="target/release/libmimalloc.so"
    fi
    test -f "$so"
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib $out/include

    so="target/${stdenv.hostPlatform.rust.cargoShortTarget}/release/libmimalloc.so"
    archive="target/${stdenv.hostPlatform.rust.cargoShortTarget}/release/libmimalloc.a"
    if [ ! -f "$so" ]; then
      so="target/release/libmimalloc.so"
      archive="target/release/libmimalloc.a"
    fi

    cp "$so" $out/lib/libmimalloc.so.3
    ln -s libmimalloc.so.3 $out/lib/libmimalloc.so
    if [ -f "$archive" ]; then
      cp "$archive" $out/lib/libmimalloc.a
    fi

    cp ${../include}/mimalloc.h $out/include/
    cp ${../include}/mimalloc-stats.h $out/include/
    cp ${../include}/mimalloc-override.h $out/include/
    cp ${../include}/mimalloc-new-delete.h $out/include/
    cp -R ${../include}/mimalloc $out/include/
    runHook postInstall
  '';

  meta = with lib; {
    description = "Pure-Rust rewrite of mimalloc with a C ABI drop-in";
    homepage = "https://github.com/microsoft/mimalloc";
    license = licenses.mit;
    platforms = platforms.linux;
  };
}
