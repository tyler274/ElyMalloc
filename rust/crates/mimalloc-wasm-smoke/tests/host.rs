#[test]
fn global_alloc_smoke() {
    assert_eq!(mimalloc_wasm_smoke::run(), 0);
}
