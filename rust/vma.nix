{
  lib,
  rustPlatform,
  stdenv,
  binutils,
  cargoTarget ? null,
  targetCc ? stdenv.cc,
}:

let
  isMusl = cargoTarget != null || stdenv.hostPlatform.isMusl;
  target = if cargoTarget != null then cargoTarget else stdenv.hostPlatform.rust.rustcTarget;
  targetFlag = lib.optionalString (cargoTarget != null) "--target ${cargoTarget}";
  cargoEnvTarget = lib.toUpper (builtins.replaceStrings [ "-" ] [ "_" ] target);
  ccBin = "${targetCc}/bin/${targetCc.targetPrefix}cc";
in
rustPlatform.buildRustPackage {
  pname = "VulkanMemoryAllocator";
  version = "3.4.0";

  src = ./.;

  cargoLock.lockFile = ./Cargo.lock;

  auditable = false;

  preBuild = lib.optionalString (cargoTarget != null) ''
    export CARGO_BUILD_TARGET=${cargoTarget}
  '';
  buildPhase = ''
    runHook preBuild
    cargo build --offline --release ${targetFlag} -p vma-c
    runHook postBuild
  '';

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
    cargo test --offline -p vma-core --release ${targetFlag}
    cargo build --release -p vma-c ${targetFlag}

    so="target/${target}/release/libVulkanMemoryAllocator.so"
    if [ ! -f "$so" ]; then
      so="target/release/libVulkanMemoryAllocator.so"
    fi
    if [ ! -f "$so" ]; then
      echo "libVulkanMemoryAllocator.so was not produced" >&2
      exit 1
    fi
    export CC=${lib.escapeShellArg ccBin}
    export VMA_SO="$(pwd)/$so"
    cargo run --offline --release ${targetFlag} -p mimalloc-harness -- vma
    runHook postCheck
  '';

  dontCargoInstall = true;

  postBuild = ''
    so="target/${target}/release/libVulkanMemoryAllocator.so"
    if [ ! -f "$so" ]; then
      so="target/release/libVulkanMemoryAllocator.so"
    fi
    if [ ! -f "$so" ]; then
      echo "libVulkanMemoryAllocator.so was not produced" >&2
      exit 1
    fi
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib $out/include

    so="target/${target}/release/libVulkanMemoryAllocator.so"
    archive="target/${target}/release/libVulkanMemoryAllocator.a"
    if [ ! -f "$so" ]; then
      so="target/release/libVulkanMemoryAllocator.so"
      archive="target/release/libVulkanMemoryAllocator.a"
    fi
    cp "$so" $out/lib/libVulkanMemoryAllocator.so.3
    ln -s libVulkanMemoryAllocator.so.3 $out/lib/libVulkanMemoryAllocator.so
    if [ -f "$archive" ]; then
      cp "$archive" $out/lib/libVulkanMemoryAllocator.a
    fi
    cp ${./crates/vma-c/include}/vk_mem_alloc.h $out/include/

    mkdir -p $out/lib/pkgconfig
    cat > $out/lib/pkgconfig/VulkanMemoryAllocator.pc <<EOF
prefix=$out
libdir=''${prefix}/lib
includedir=''${prefix}/include

Name: VulkanMemoryAllocator
Description: Pure-Rust Vulkan Memory Allocator (AMD VMA 3.4 ABI)
Version: 3.4.0
Libs: -L''${libdir} -lVulkanMemoryAllocator
Cflags: -I''${includedir}
EOF
    runHook postInstall
  '';

  meta = with lib; {
    description = "Pure-Rust Vulkan Memory Allocator with AMD VMA 3.4 C ABI";
    homepage = "https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator";
    license = licenses.mit;
    platforms = platforms.linux;
  };
}
