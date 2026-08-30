use anyhow::Result;

use crate::env_is_one;
use crate::process::cargo_ok;

pub fn run() -> Result<()> {
    cargo_ok(&["test", "-p", "mimalloc-harness"])?;
    cargo_ok(&["test", "-p", "mimalloc-core"])?;
    cargo_ok(&["test", "-p", "mimalloc-wasm-smoke"])?;
    cargo_ok(&["build", "--release", "-p", "mimalloc-c"])?;
    cargo_ok(&["build", "-p", "mimalloc-c"])?;

    let rust = crate::rust_root();
    let repo = crate::repo_root();
    std::env::set_var("SO", rust.join("target/release/libmimalloc.so"));
    std::env::set_var("DEBUG_SO", rust.join("target/debug/libmimalloc.so"));
    std::env::set_var("INCLUDE", repo.join("include"));
    std::env::set_var("C_TESTS", rust.join("tests"));
    std::env::set_var("UPSTREAM_TESTS", repo.join("test"));
    crate::cabi::run()?;

    if !env_is_one("SKIP_WASM_SMOKE") {
        crate::wasm_smoke::run()?;
    }

    println!("all rust mimalloc checks passed");
    Ok(())
}
