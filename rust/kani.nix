{
  lib,
  stdenv,
  fetchurl,
  autoPatchelfHook,
  zlib,
  rust-bin,
}:

let
  version = "0.67.0";
  # Must match the bundle's `rust-toolchain-version` (Kani 0.67.0).
  rustDate = "2025-11-21";
  triple = stdenv.hostPlatform.rust.rustcTarget;
  hashes = {
    x86_64-unknown-linux-gnu = "sha256-O196/TtRYD7nINt7wbxP5GtaT1022q2ZOcS0xli1GsA=";
    aarch64-unknown-linux-gnu = "sha256-l0Eo9E3UNhigbSHl/m2f9nGI3lmG/gvFe1NLDkY577k=";
  };
  rustcNightly = rust-bin.nightly.${rustDate}.default.override {
    extensions = [
      "rust-src"
      "rustc-dev"
      "llvm-tools"
    ];
  };
in
stdenv.mkDerivation {
  pname = "kani";
  inherit version;

  src = fetchurl {
    url = "https://github.com/model-checking/kani/releases/download/kani-${version}/kani-${version}-${triple}.tar.gz";
    hash = hashes.${triple} or (throw "kani ${version}: no release bundle for ${triple}");
  };

  sourceRoot = "kani-${version}";

  nativeBuildInputs = [ autoPatchelfHook ];
  buildInputs = [
    zlib
    stdenv.cc.cc
    rustcNightly
  ];

  dontStrip = true;
  dontPatchELF = false;

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    $out/bin/cargo-kani --version
    test -x $out/toolchain/bin/cargo
    test -e $out/toolchain/lib/librustc_driver-*.so
    runHook postInstallCheck
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -a . $out/
    rm -f $out/toolchain
    ln -s ${rustcNightly} $out/toolchain
    ln -sfn kani-driver $out/bin/cargo-kani
    ln -sfn kani-driver $out/bin/kani
    runHook postInstall
  '';

  # Keep $ORIGIN/../toolchain/lib so kani-compiler finds librustc_driver
  # from the rust-overlay nightly, not a rustup path baked into the bundle.
  appendRunpaths = [
    "${placeholder "out"}/toolchain/lib"
    "${rustcNightly}/lib"
  ];

  meta = {
    description = "Kani Rust Verifier (official GitHub release bundle)";
    homepage = "https://github.com/model-checking/kani";
    license = with lib.licenses; [
      mit
      asl20
    ];
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
    mainProgram = "cargo-kani";
  };
}
