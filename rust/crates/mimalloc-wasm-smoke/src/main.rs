//! Host/WASI entry uses std. `wasm32-unknown-unknown` is `no_std` + exported `smoke`.
#![cfg_attr(all(target_arch = "wasm32", not(target_os = "wasi")), no_std, no_main)]

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
#[no_mangle]
pub extern "C" fn smoke() -> i32 {
    mimalloc_wasm_smoke::run()
}

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
#[no_mangle]
pub extern "C" fn _start() {
    if mimalloc_wasm_smoke::run() != 0 {
        core::arch::wasm32::unreachable();
    }
}

#[cfg(any(not(target_arch = "wasm32"), target_os = "wasi"))]
fn main() {
    let rc = mimalloc_wasm_smoke::run();
    if rc != 0 {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("mimalloc-wasm-smoke failed: {rc}");
        std::process::exit(rc);
    }
    println!("mimalloc-wasm-smoke ok");
}
