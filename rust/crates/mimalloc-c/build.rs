fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libmimalloc.so.3");
        println!("cargo:rustc-link-arg=-Wl,-soname,libmimalloc.so.3");
    }
    println!("cargo:rustc-link-lib=pthread");
    println!("cargo:rustc-link-lib=dl");
}
