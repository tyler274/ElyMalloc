//! Host and WASI use std `main`. `wasm32-unknown-unknown` also has std
//! (`any_spawner` / `reactive_graph`); export `smoke` for wasmtime.

#[no_mangle]
pub extern "C" fn smoke() -> i32 {
    mimalloc_leptos_smoke::run()
}

fn main() {
    let rc = mimalloc_leptos_smoke::run();
    if rc != 0 {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("mimalloc-leptos-smoke failed: {rc}");
        std::process::exit(rc);
    }
    println!("mimalloc-leptos-smoke ok");
}
