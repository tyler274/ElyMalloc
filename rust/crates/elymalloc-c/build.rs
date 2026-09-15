//! SONAME / install_name / CRT link flags. GNU `--no-as-needed -lc` is Linux-only.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let secure = std::env::var("CARGO_FEATURE_SECURE").is_ok();
    match os.as_str() {
        "linux" => {
            let soname = if secure {
                "libmimalloc-secure.so.3"
            } else {
                "libmimalloc.so.3"
            };
            println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,{soname}");
            println!("cargo:rustc-link-arg=-Wl,-soname,{soname}");
            if env != "musl" {
                println!("cargo:rustc-link-lib=pthread");
                println!("cargo:rustc-link-lib=dl");
                println!("cargo:rustc-cdylib-link-arg=-Wl,--no-as-needed");
                println!("cargo:rustc-cdylib-link-arg=-lc");
            }
        }
        "macos" => {
            let name = if secure {
                "libmimalloc-secure.3.dylib"
            } else {
                "libmimalloc.3.dylib"
            };
            println!("cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/{name}");
        }
        _ => {}
    }
}
