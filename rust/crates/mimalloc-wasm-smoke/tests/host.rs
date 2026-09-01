#[test]
fn global_alloc_smoke() {
    assert_eq!(mimalloc_wasm_smoke::run(), 0);
}

#[test]
fn global_alloc_stress() {
    assert_eq!(mimalloc_wasm_smoke::run_stress(), 0);
}
