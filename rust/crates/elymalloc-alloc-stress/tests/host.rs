#[test]
fn global_allocator_stress() {
    assert_eq!(elymalloc_alloc_stress::run(), 0);
}
