#[test]
fn global_allocator_stress() {
    assert_eq!(mimalloc_alloc_stress::run(), 0);
}
