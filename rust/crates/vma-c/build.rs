fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "linux" {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libVulkanMemoryAllocator.so.3");
        println!("cargo:rustc-link-arg=-Wl,-soname,libVulkanMemoryAllocator.so.3");
    }
    // musl folds pthread and dl into libc; glibc still needs the extra libs.
    if env != "musl" {
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=dl");
    }
}
