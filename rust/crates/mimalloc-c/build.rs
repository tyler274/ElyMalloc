//! SONAME and glibc `DT_NEEDED libc.so.6` (`--no-as-needed -lc`).
//!
//! rustc cdylibs allow undefined symbols, so `-lc --as-needed` is dropped and
//! glibc-only stubs (`atexit`) stay `U`. Force `-lc` here; `lib.rs` registers
//! teardown with `__cxa_atexit`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "linux" {
        let soname = if std::env::var("CARGO_FEATURE_SECURE").is_ok() {
            "libmimalloc-secure.so.3"
        } else {
            "libmimalloc.so.3"
        };
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,{soname}");
        println!("cargo:rustc-link-arg=-Wl,-soname,{soname}");
    }
    // musl folds pthread and dl into libc; glibc still needs the extra libs.
    if env != "musl" {
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-cdylib-link-arg=-Wl,--no-as-needed");
        println!("cargo:rustc-cdylib-link-arg=-lc");
    }
}
