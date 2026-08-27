//! C ABI and libc malloc override (`cdylib` / `staticlib`).
//!
//! Exports both `mi_*` names and the standard allocator symbols so the
//! resulting `libmimalloc.so` can be used with `LD_PRELOAD` or as a
//! drop-in for NixOS `environment.memoryAllocator.provider = "mimalloc"`.

#![cfg_attr(not(test), no_std)]
#![allow(non_snake_case)]

use core::ffi::c_void;
use mimalloc_core::alloc as mi;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    mimalloc_core_abort()
}

fn mimalloc_core_abort() -> ! {
    unsafe { libc::_exit(1) }
}

#[inline]
fn pvoid(p: *mut u8) -> *mut c_void {
    p as *mut c_void
}

#[inline]
fn pu8(p: *mut c_void) -> *mut u8 {
    p as *mut u8
}

// ---------------------------------------------------------------------------
// mimalloc-prefixed API
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn mi_malloc(size: usize) -> *mut c_void {
    pvoid(mi::malloc(size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_calloc(count: usize, size: usize) -> *mut c_void {
    pvoid(mi::calloc(count, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_realloc(p: *mut c_void, newsize: usize) -> *mut c_void {
    pvoid(mi::realloc(pu8(p), newsize))
}

#[no_mangle]
pub unsafe extern "C" fn mi_reallocf(p: *mut c_void, newsize: usize) -> *mut c_void {
    pvoid(mi::reallocf(pu8(p), newsize))
}

#[no_mangle]
pub unsafe extern "C" fn mi_free(p: *mut c_void) {
    mi::free(pu8(p));
}

#[no_mangle]
pub unsafe extern "C" fn mi_expand(_p: *mut c_void, _newsize: usize) -> *mut c_void {
    core::ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn mi_malloc_small(size: usize) -> *mut c_void {
    mi_malloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_zalloc_small(size: usize) -> *mut c_void {
    mi_zalloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_zalloc(size: usize) -> *mut c_void {
    pvoid(mi::calloc(1, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_mallocn(count: usize, size: usize) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_malloc(total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_usable_size(p: *const c_void) -> usize {
    mi::usable_size(p as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn mi_good_size(size: usize) -> usize {
    mi::good_size(size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_free_size(p: *mut c_void, _size: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn mi_free_small(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn mi_malloc_aligned(size: usize, alignment: usize) -> *mut c_void {
    pvoid(mi::malloc_aligned(size, alignment))
}

#[no_mangle]
pub unsafe extern "C" fn mi_zalloc_aligned(size: usize, alignment: usize) -> *mut c_void {
    let p = mi::malloc_aligned(size, alignment);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, size);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_calloc_aligned(
    count: usize,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_zalloc_aligned(total, alignment)
}

#[no_mangle]
pub unsafe extern "C" fn mi_realloc_aligned(
    p: *mut c_void,
    newsize: usize,
    alignment: usize,
) -> *mut c_void {
    if p.is_null() {
        return mi_malloc_aligned(newsize, alignment);
    }
    if newsize == 0 {
        mi_free(p);
        return mi_malloc_aligned(0, alignment);
    }
    let old = mi::usable_size(p as *const u8);
    if old >= newsize && (p as usize) % alignment == 0 {
        return p;
    }
    let q = mi::malloc_aligned(newsize, alignment);
    if q.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(p as *const u8, q, old.min(newsize));
    mi::free(pu8(p));
    pvoid(q)
}

#[no_mangle]
pub unsafe extern "C" fn mi_strdup(s: *const libc::c_char) -> *mut libc::c_char {
    mi::strdup(s)
}

#[no_mangle]
pub unsafe extern "C" fn mi_strndup(s: *const libc::c_char, n: usize) -> *mut libc::c_char {
    mi::strndup(s, n)
}

#[no_mangle]
pub unsafe extern "C" fn mi_version() -> i32 {
    mi::VERSION
}

#[no_mangle]
pub unsafe extern "C" fn mi_collect(force: bool) {
    mi::collect(force);
}

#[no_mangle]
pub unsafe extern "C" fn mi_process_init() {
    mimalloc_core::init();
}

#[no_mangle]
pub unsafe extern "C" fn mi_process_done() {}

#[no_mangle]
pub unsafe extern "C" fn mi_thread_init() {
    mimalloc_core::init();
}

#[no_mangle]
pub unsafe extern "C" fn mi_thread_done() {}

#[no_mangle]
pub unsafe extern "C" fn mi_cfree(p: *mut c_void) -> bool {
    if p.is_null() {
        return true;
    }
    if mi::usable_size(p as *const u8) == 0 {
        return false;
    }
    mi::free(pu8(p));
    true
}

#[no_mangle]
pub unsafe extern "C" fn mi_malloc_size(p: *const c_void) -> usize {
    mi::usable_size(p as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn mi_malloc_usable_size(p: *const c_void) -> usize {
    mi::usable_size(p as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn mi_malloc_good_size(size: usize) -> usize {
    mi::good_size(size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_posix_memalign(p: *mut *mut c_void, alignment: usize, size: usize) -> i32 {
    // POSIX: do not modify `*p` on error (EINVAL / ENOMEM).
    if p.is_null() {
        return libc::EINVAL;
    }
    let mut q: *mut u8 = core::ptr::null_mut();
    let rc = mi::posix_memalign(&mut q, alignment, size);
    if rc == 0 {
        *p = q as *mut c_void;
    }
    rc
}

#[no_mangle]
pub unsafe extern "C" fn mi_memalign(alignment: usize, size: usize) -> *mut c_void {
    pvoid(mi::memalign(alignment, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_valloc(size: usize) -> *mut c_void {
    pvoid(mi::valloc(size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_pvalloc(size: usize) -> *mut c_void {
    pvoid(mi::pvalloc(size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_aligned_alloc(alignment: usize, size: usize) -> *mut c_void {
    pvoid(mi::aligned_alloc(alignment, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_reallocarray(p: *mut c_void, count: usize, size: usize) -> *mut c_void {
    pvoid(mi::reallocarray(pu8(p), count, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_reallocarr(ptrp: *mut *mut c_void, count: usize, size: usize) -> i32 {
    mi::reallocarr(ptrp as *mut *mut u8, count, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new(size: usize) -> *mut c_void {
    let p = mi::malloc(size);
    if p.is_null() {
        mimalloc_core_abort();
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_nothrow(size: usize) -> *mut c_void {
    mi_malloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_aligned(size: usize, alignment: usize) -> *mut c_void {
    let p = mi::malloc_aligned(size, alignment);
    if p.is_null() {
        mimalloc_core_abort();
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_aligned_nothrow(size: usize, alignment: usize) -> *mut c_void {
    mi_malloc_aligned(size, alignment)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_n(count: usize, size: usize) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        mimalloc_core_abort();
    };
    mi_new(total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_malloc_aligned_at(
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    pvoid(mi::malloc_aligned_at(size, alignment, offset))
}

#[no_mangle]
pub unsafe extern "C" fn mi_zalloc_aligned_at(
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    let p = mi::malloc_aligned_at(size, alignment, offset);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, size);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_rezalloc(p: *mut c_void, newsize: usize) -> *mut c_void {
    pvoid(mi::rezalloc(pu8(p), newsize))
}

#[no_mangle]
pub unsafe extern "C" fn mi_recalloc(p: *mut c_void, newcount: usize, size: usize) -> *mut c_void {
    let Some(total) = newcount.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_rezalloc(p, total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_rezalloc_aligned(
    p: *mut c_void,
    newsize: usize,
    alignment: usize,
) -> *mut c_void {
    pvoid(mi::rezalloc_aligned(pu8(p), newsize, alignment))
}

#[no_mangle]
pub unsafe extern "C" fn mi_umalloc(size: usize, block_size: *mut usize) -> *mut c_void {
    pvoid(mi::umalloc(size, block_size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_ucalloc(
    count: usize,
    size: usize,
    block_size: *mut usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        if !block_size.is_null() {
            *block_size = 0;
        }
        return core::ptr::null_mut();
    };
    let p = mi::umalloc(total, block_size);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, total);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_urealloc(
    p: *mut c_void,
    newsize: usize,
    pre: *mut usize,
    post: *mut usize,
) -> *mut c_void {
    pvoid(mi::urealloc(pu8(p), newsize, pre, post))
}

#[no_mangle]
pub unsafe extern "C" fn mi_ufree(p: *mut c_void, block_size: *mut usize) {
    mi::ufree(pu8(p), block_size);
}

#[no_mangle]
pub unsafe extern "C" fn mi_realpath(
    fname: *const libc::c_char,
    resolved_name: *mut libc::c_char,
) -> *mut libc::c_char {
    mi::realpath(fname, resolved_name)
}

#[no_mangle]
pub unsafe extern "C" fn mi_reserve_os_memory(size: usize, commit: bool, allow_large: bool) -> i32 {
    mi::reserve_os_memory(size, commit, allow_large)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_new() -> *mut mimalloc_core::Heap {
    mimalloc_core::heap_new()
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_delete(heap: *mut mimalloc_core::Heap) {
    mimalloc_core::heap_delete(heap);
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_destroy(heap: *mut mimalloc_core::Heap) {
    mimalloc_core::heap_destroy(heap);
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_collect(heap: *mut mimalloc_core::Heap, force: bool) {
    mimalloc_core::heap_collect(heap, force);
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_malloc(heap: *mut mimalloc_core::Heap, size: usize) -> *mut c_void {
    pvoid(mimalloc_core::heap_malloc(heap, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_zalloc(heap: *mut mimalloc_core::Heap, size: usize) -> *mut c_void {
    let p = mimalloc_core::heap_malloc(heap, size);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, size);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_calloc(
    heap: *mut mimalloc_core::Heap,
    count: usize,
    size: usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_heap_zalloc(heap, total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_malloc_aligned(
    heap: *mut mimalloc_core::Heap,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    pvoid(mimalloc_core::heap_malloc_aligned(heap, size, alignment))
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_realloc(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newsize: usize,
) -> *mut c_void {
    if p.is_null() {
        return mi_heap_malloc(heap, newsize);
    }
    let old = mi::usable_size(p as *const u8);
    if old >= newsize {
        return p;
    }
    let q = mimalloc_core::heap_malloc(heap, newsize);
    if q.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(p as *const u8, q, old.min(newsize));
    mi::free(pu8(p));
    pvoid(q)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_malloc_small(
    heap: *mut mimalloc_core::Heap,
    size: usize,
) -> *mut c_void {
    mi_heap_malloc(heap, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_zalloc_small(
    heap: *mut mimalloc_core::Heap,
    size: usize,
) -> *mut c_void {
    mi_heap_zalloc(heap, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_mallocn(
    heap: *mut mimalloc_core::Heap,
    count: usize,
    size: usize,
) -> *mut c_void {
    mi_heap_calloc(heap, count, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_alloc_new(
    heap: *mut mimalloc_core::Heap,
    size: usize,
) -> *mut c_void {
    mi_heap_malloc(heap, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_alloc_new_n(
    heap: *mut mimalloc_core::Heap,
    count: usize,
    size: usize,
) -> *mut c_void {
    mi_heap_mallocn(heap, count, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_main() -> *mut mimalloc_core::Heap {
    mimalloc_core::heap_main()
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_of(p: *const c_void) -> *mut mimalloc_core::Heap {
    mimalloc_core::heap_of(p as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_contains(heap: *const mimalloc_core::Heap, p: *const c_void) -> bool {
    mimalloc_core::heap_contains(heap, p as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn mi_any_heap_contains(p: *const c_void) -> bool {
    mimalloc_core::any_heap_contains(p as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn mi_check_owned(p: *const c_void) -> bool {
    mi_any_heap_contains(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_set_numa_affinity(_heap: *mut mimalloc_core::Heap, _numa_node: i32) {}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_theap(heap: *mut mimalloc_core::Heap) -> *mut mimalloc_core::Theap {
    mimalloc_core::heap_theap(heap)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_get_default() -> *mut mimalloc_core::Theap {
    mimalloc_core::theap_get_default()
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_set_default(theap: *mut mimalloc_core::Theap) -> *mut mimalloc_core::Theap {
    mimalloc_core::theap_set_default(theap)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_collect(theap: *mut mimalloc_core::Theap, force: bool) {
    mimalloc_core::theap_collect(theap, force);
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_malloc(theap: *mut mimalloc_core::Theap, size: usize) -> *mut c_void {
    pvoid(mimalloc_core::theap_malloc(theap, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_zalloc(theap: *mut mimalloc_core::Theap, size: usize) -> *mut c_void {
    let p = mimalloc_core::theap_malloc(theap, size);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, size);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_calloc(
    theap: *mut mimalloc_core::Theap,
    count: usize,
    size: usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_theap_zalloc(theap, total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_malloc_small(
    theap: *mut mimalloc_core::Theap,
    size: usize,
) -> *mut c_void {
    mi_theap_malloc(theap, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_zalloc_small(
    theap: *mut mimalloc_core::Theap,
    size: usize,
) -> *mut c_void {
    mi_theap_zalloc(theap, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_malloc_aligned(
    theap: *mut mimalloc_core::Theap,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    pvoid(mimalloc_core::theap_malloc_aligned(theap, size, alignment))
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_zalloc_aligned(
    theap: *mut mimalloc_core::Theap,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    let p = mimalloc_core::theap_malloc_aligned(theap, size, alignment);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, size);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_realloc(
    theap: *mut mimalloc_core::Theap,
    p: *mut c_void,
    newsize: usize,
) -> *mut c_void {
    if p.is_null() {
        return mi_theap_malloc(theap, newsize);
    }
    let old = mi::usable_size(p as *const u8);
    if old >= newsize {
        return p;
    }
    let q = mimalloc_core::theap_malloc(theap, newsize);
    if q.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(p as *const u8, q, old.min(newsize));
    mi::free(pu8(p));
    pvoid(q)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_rezalloc(
    theap: *mut mimalloc_core::Theap,
    p: *mut c_void,
    newsize: usize,
) -> *mut c_void {
    if p.is_null() {
        return mi_theap_zalloc(theap, newsize);
    }
    let old = mi::usable_size(p as *const u8);
    if old >= newsize {
        return p;
    }
    let q = mi_theap_realloc(theap, p, newsize);
    if !q.is_null() && newsize > old {
        core::ptr::write_bytes((q as *mut u8).add(old), 0, newsize - old);
    }
    q
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_guarded_set_sample_rate(
    _theap: *mut mimalloc_core::Theap,
    _sample_rate: usize,
    _seed: usize,
) {
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_guarded_set_size_bound(
    _theap: *mut mimalloc_core::Theap,
    _min: usize,
    _max: usize,
) {
}

#[no_mangle]
pub unsafe extern "C" fn mi_reserve_os_memory_ex(
    size: usize,
    commit: bool,
    allow_large: bool,
    exclusive: bool,
    arena_id: *mut *mut c_void,
) -> i32 {
    let mut a: *mut mimalloc_core::Arena = core::ptr::null_mut();
    let rc = mi::reserve_os_memory_ex(size, commit, allow_large, exclusive, &mut a);
    if !arena_id.is_null() && rc == 0 {
        *arena_id = a as *mut c_void;
    }
    rc
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_new_in_arena(arena_id: *mut c_void) -> *mut mimalloc_core::Heap {
    mimalloc_core::heap_new_in_arena(arena_id as *mut mimalloc_core::Arena)
}

#[no_mangle]
pub unsafe extern "C" fn mi_arena_area(arena_id: *mut c_void, size: *mut usize) -> *mut c_void {
    mimalloc_core::mi_arena::area(arena_id as *const mimalloc_core::Arena, size) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn mi_arena_contains(arena_id: *mut c_void, p: *const c_void) -> bool {
    mimalloc_core::mi_arena::contains(arena_id as *const mimalloc_core::Arena, p as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn mi_arena_min_alignment() -> usize {
    mimalloc_core::mi_arena::ARENA_MIN_ALIGN
}

#[no_mangle]
pub unsafe extern "C" fn mi_arena_max_object_size() -> usize {
    mimalloc_core::LARGE_MAX_OBJ_SIZE
}

#[no_mangle]
pub unsafe extern "C" fn mi_is_redirected() -> bool {
    false
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_visit_blocks(
    heap: *mut mimalloc_core::Heap,
    visit_blocks: bool,
    visitor: Option<mimalloc_core::BlockVisitFun>,
    arg: *mut c_void,
) -> bool {
    mimalloc_core::heap_visit_blocks(heap, visit_blocks, visitor, arg)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_visit_abandoned_blocks(
    heap: *mut mimalloc_core::Heap,
    visit_blocks: bool,
    visitor: Option<mimalloc_core::BlockVisitFun>,
    arg: *mut c_void,
) -> bool {
    mimalloc_core::heap_visit_abandoned_blocks(heap, visit_blocks, visitor, arg)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_visit_blocks(
    theap: *const mimalloc_core::Theap,
    visit_blocks: bool,
    visitor: Option<mimalloc_core::BlockVisitFun>,
    arg: *mut c_void,
) -> bool {
    mimalloc_core::theap_visit_blocks(theap as *mut mimalloc_core::Theap, visit_blocks, visitor, arg)
}

#[no_mangle]
pub unsafe extern "C" fn mi_manage_os_memory(
    start: *mut c_void,
    size: usize,
    is_committed: bool,
    is_pinned: bool,
    is_zero: bool,
    numa_node: i32,
) -> bool {
    mi_manage_os_memory_ex(
        start,
        size,
        is_committed,
        is_pinned,
        is_zero,
        numa_node,
        false,
        core::ptr::null_mut(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn mi_manage_os_memory_ex(
    start: *mut c_void,
    size: usize,
    is_committed: bool,
    is_pinned: bool,
    is_zero: bool,
    numa_node: i32,
    exclusive: bool,
    arena_id: *mut *mut c_void,
) -> bool {
    let mut a: *mut mimalloc_core::Arena = core::ptr::null_mut();
    let ok = mi::manage_os_memory_ex(
        start as *mut u8,
        size,
        is_committed,
        is_pinned,
        is_zero,
        numa_node,
        exclusive,
        &mut a,
    );
    if ok && !arena_id.is_null() {
        *arena_id = a as *mut c_void;
    }
    ok
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_main() -> mimalloc_core::SubprocId {
    mimalloc_core::mi_subproc::main()
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_current() -> mimalloc_core::SubprocId {
    mimalloc_core::mi_subproc::current()
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_new() -> mimalloc_core::SubprocId {
    mimalloc_core::mi_subproc::new()
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_destroy(subproc: mimalloc_core::SubprocId) {
    mimalloc_core::mi_subproc::destroy(subproc);
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_add_current_thread(subproc: mimalloc_core::SubprocId) {
    mimalloc_core::mi_subproc::add_current_thread(subproc);
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_visit_heaps(
    subproc: mimalloc_core::SubprocId,
    visitor: Option<mimalloc_core::mi_subproc::HeapVisitFun>,
    arg: *mut c_void,
) -> bool {
    mimalloc_core::mi_subproc::visit_heaps(subproc, visitor, arg)
}

#[no_mangle]
pub unsafe extern "C" fn mi_reserve_huge_os_pages_at(
    pages: usize,
    _numa_node: i32,
    _timeout_msecs: usize,
) -> i32 {
    mi_reserve_huge_os_pages_at_ex(pages, _numa_node, _timeout_msecs, false, core::ptr::null_mut())
}

#[no_mangle]
pub unsafe extern "C" fn mi_reserve_huge_os_pages_interleave(
    pages: usize,
    _numa_nodes: usize,
    timeout_msecs: usize,
) -> i32 {
    mi_reserve_huge_os_pages_at(pages, -1, timeout_msecs)
}

#[no_mangle]
pub unsafe extern "C" fn mi_reserve_huge_os_pages_at_ex(
    pages: usize,
    _numa_node: i32,
    _timeout_msecs: usize,
    exclusive: bool,
    arena_id: *mut *mut c_void,
) -> i32 {
    let size = pages.saturating_mul(1024 * 1024 * 1024);
    mi_reserve_os_memory_ex(size, true, true, exclusive, arena_id)
}

#[no_mangle]
pub unsafe extern "C" fn mi_reserve_huge_os_pages(
    pages: usize,
    _max_secs: f64,
    pages_reserved: *mut usize,
) -> i32 {
    let rc = mi_reserve_huge_os_pages_at(pages, -1, 0);
    if !pages_reserved.is_null() {
        *pages_reserved = if rc == 0 { pages } else { 0 };
    }
    rc
}

#[no_mangle]
pub unsafe extern "C" fn mi_register_deferred_free(_f: *mut c_void, _arg: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn mi_register_output(_f: *mut c_void, _arg: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn mi_register_error(_f: *mut c_void, _arg: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn mi_thread_set_in_threadpool() {}

#[no_mangle]
pub unsafe extern "C" fn mi_collect_reduce(_target: usize) {
    mi_collect(true);
}

#[no_mangle]
pub unsafe extern "C" fn mi_process_info(
    elapsed_msecs: *mut usize,
    user_msecs: *mut usize,
    system_msecs: *mut usize,
    current_rss: *mut usize,
    peak_rss: *mut usize,
    current_commit: *mut usize,
    peak_commit: *mut usize,
    page_faults: *mut usize,
) {
    let mut ru: libc::rusage = core::mem::zeroed();
    libc::getrusage(libc::RUSAGE_SELF, &mut ru);
    let user = (ru.ru_utime.tv_sec as usize)
        .saturating_mul(1000)
        .saturating_add((ru.ru_utime.tv_usec as usize) / 1000);
    let sys = (ru.ru_stime.tv_sec as usize)
        .saturating_mul(1000)
        .saturating_add((ru.ru_stime.tv_usec as usize) / 1000);
    let rss = (ru.ru_maxrss as usize).saturating_mul(1024);
    let faults = ru.ru_majflt as usize;
    if !elapsed_msecs.is_null() {
        *elapsed_msecs = user.saturating_add(sys);
    }
    if !user_msecs.is_null() {
        *user_msecs = user;
    }
    if !system_msecs.is_null() {
        *system_msecs = sys;
    }
    if !current_rss.is_null() {
        *current_rss = rss;
    }
    if !peak_rss.is_null() {
        *peak_rss = rss;
    }
    if !current_commit.is_null() {
        *current_commit = rss;
    }
    if !peak_commit.is_null() {
        *peak_commit = rss;
    }
    if !page_faults.is_null() {
        *page_faults = faults;
    }
}

#[no_mangle]
pub unsafe extern "C" fn mi_process_info_print() {}

#[no_mangle]
pub unsafe extern "C" fn mi_process_info_print_out(_out: *mut c_void, _arg: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn mi_options_print_out(_out: *mut c_void, _arg: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn mi_arenas_print() {}

#[no_mangle]
pub unsafe extern "C" fn mi_reallocn(p: *mut c_void, count: usize, size: usize) -> *mut c_void {
    mi_reallocarray(p, count, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_calloc_aligned_at(
    count: usize,
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    let p = mi::malloc_aligned_at(total, alignment, offset);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, total);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_realloc_aligned_at(
    p: *mut c_void,
    newsize: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    if p.is_null() {
        return pvoid(mi::malloc_aligned_at(newsize, alignment, offset));
    }
    let old = mi::usable_size(p as *const u8);
    if old >= newsize {
        return p;
    }
    let q = mi::malloc_aligned_at(newsize, alignment, offset);
    if q.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(p as *const u8, q, old.min(newsize));
    mi::free(pu8(p));
    pvoid(q)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_strdup(
    heap: *mut mimalloc_core::Heap,
    s: *const libc::c_char,
) -> *mut libc::c_char {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    let n = libc::strlen(s);
    let d = mimalloc_core::heap_malloc(heap, n + 1) as *mut libc::c_char;
    if !d.is_null() {
        core::ptr::copy_nonoverlapping(s, d, n + 1);
    }
    d
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_reallocn(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    count: usize,
    size: usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_heap_realloc(heap, p, total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_reallocf(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newsize: usize,
) -> *mut c_void {
    let q = mi_heap_realloc(heap, p, newsize);
    if q.is_null() && !p.is_null() && newsize != 0 {
        mi_free(p);
    }
    q
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_zalloc_aligned(
    heap: *mut mimalloc_core::Heap,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    let p = mimalloc_core::heap_malloc_aligned(heap, size, alignment);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, size);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_rezalloc(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newsize: usize,
) -> *mut c_void {
    if p.is_null() {
        return mi_heap_zalloc(heap, newsize);
    }
    let old = mi::usable_size(p as *const u8);
    let q = mi_heap_realloc(heap, p, newsize);
    if !q.is_null() && newsize > old {
        core::ptr::write_bytes((q as *mut u8).add(old), 0, newsize - old);
    }
    q
}

#[no_mangle]
pub unsafe extern "C" fn mi_umalloc_aligned(
    size: usize,
    alignment: usize,
    block_size: *mut usize,
) -> *mut c_void {
    let p = mi::malloc_aligned(size, alignment);
    if !block_size.is_null() {
        *block_size = if p.is_null() {
            0
        } else {
            mi::usable_size(p as *const u8)
        };
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_uzalloc_aligned(
    size: usize,
    alignment: usize,
    block_size: *mut usize,
) -> *mut c_void {
    let p = mi_umalloc_aligned(size, alignment, block_size);
    if !p.is_null() {
        core::ptr::write_bytes(p as *mut u8, 0, size);
    }
    p
}

#[no_mangle]
pub unsafe extern "C" fn mi_umalloc_small(size: usize, block_size: *mut usize) -> *mut c_void {
    mi_umalloc(size, block_size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_uzalloc_small(size: usize, block_size: *mut usize) -> *mut c_void {
    let p = mi_umalloc(size, block_size);
    if !p.is_null() {
        core::ptr::write_bytes(p as *mut u8, 0, size);
    }
    p
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_realloc(p: *mut c_void, newsize: usize) -> *mut c_void {
    mi_realloc(p, newsize)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_reallocn(p: *mut c_void, count: usize, size: usize) -> *mut c_void {
    mi_reallocn(p, count, size)
}

#[no_mangle]
pub unsafe extern "C" fn mi_free_aligned(p: *mut c_void, _alignment: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_print(_out: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_print_out(_out: *mut c_void, _arg: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_reset() {
    mimalloc_core::mi_stats::reset();
}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_get(stats: *mut mimalloc_core::Stats) -> bool {
    if stats.is_null() {
        return false;
    }
    mimalloc_core::mi_stats::fill(stats);
    true
}

#[no_mangle]
pub unsafe extern "C" fn mi_options_print() {}

#[no_mangle]
pub unsafe extern "C" fn mi_debug_show_arenas() {}

#[no_mangle]
pub unsafe extern "C" fn mi_option_set(option: i32, value: libc::c_long) {
    mimalloc_core::mi_options::set(option, value as i64);
}

#[no_mangle]
pub unsafe extern "C" fn mi_option_set_default(option: i32, value: libc::c_long) {
    mimalloc_core::mi_options::set(option, value as i64);
}

#[no_mangle]
pub unsafe extern "C" fn mi_option_enable(option: i32) {
    mimalloc_core::mi_options::enable(option);
}

#[no_mangle]
pub unsafe extern "C" fn mi_option_disable(option: i32) {
    mimalloc_core::mi_options::disable(option);
}

#[no_mangle]
pub unsafe extern "C" fn mi_option_set_enabled(option: i32, enable: bool) {
    if enable {
        mimalloc_core::mi_options::enable(option);
    } else {
        mimalloc_core::mi_options::disable(option);
    }
}

#[no_mangle]
pub unsafe extern "C" fn mi_option_is_enabled(option: i32) -> bool {
    mimalloc_core::mi_options::is_enabled(option)
}

#[no_mangle]
pub unsafe extern "C" fn mi_option_get(option: i32) -> libc::c_long {
    mimalloc_core::mi_options::get(option) as libc::c_long
}

#[no_mangle]
pub unsafe extern "C" fn mi_option_get_clamp(
    option: i32,
    min: libc::c_long,
    max: libc::c_long,
) -> libc::c_long {
    mimalloc_core::mi_options::clamp(option, min as i64, max as i64) as libc::c_long
}

#[no_mangle]
pub unsafe extern "C" fn mi_option_get_size(option: i32) -> usize {
    mimalloc_core::mi_options::get_size(option)
}

#[no_mangle]
pub unsafe extern "C" fn mi_arena_min_size() -> usize {
    mimalloc_core::mi_arena::ARENA_MIN_SIZE
}

#[no_mangle]
pub unsafe extern "C" fn mi_is_in_heap_region(p: *const c_void) -> bool {
    mi::usable_size(p as *const u8) != 0 || p.is_null()
}

// ---------------------------------------------------------------------------
// libc override symbols
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    mi_malloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut c_void {
    mi_calloc(count, size)
}

#[no_mangle]
pub unsafe extern "C" fn realloc(p: *mut c_void, newsize: usize) -> *mut c_void {
    mi_realloc(p, newsize)
}

#[no_mangle]
pub unsafe extern "C" fn free(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn strdup(s: *const libc::c_char) -> *mut libc::c_char {
    mi_strdup(s)
}

#[no_mangle]
pub unsafe extern "C" fn strndup(s: *const libc::c_char, n: usize) -> *mut libc::c_char {
    mi_strndup(s, n)
}

#[no_mangle]
pub unsafe extern "C" fn reallocf(p: *mut c_void, newsize: usize) -> *mut c_void {
    mi_reallocf(p, newsize)
}

#[no_mangle]
pub unsafe extern "C" fn malloc_size(p: *const c_void) -> usize {
    mi_malloc_size(p)
}

#[no_mangle]
pub unsafe extern "C" fn malloc_usable_size(p: *mut c_void) -> usize {
    mi_malloc_usable_size(p)
}

#[no_mangle]
pub unsafe extern "C" fn malloc_good_size(size: usize) -> usize {
    mi_malloc_good_size(size)
}

#[no_mangle]
pub unsafe extern "C" fn posix_memalign(p: *mut *mut c_void, alignment: usize, size: usize) -> i32 {
    mi_posix_memalign(p, alignment, size)
}

#[no_mangle]
pub unsafe extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut c_void {
    mi_aligned_alloc(alignment, size)
}

#[no_mangle]
pub unsafe extern "C" fn memalign(alignment: usize, size: usize) -> *mut c_void {
    mi_memalign(alignment, size)
}

#[no_mangle]
pub unsafe extern "C" fn valloc(size: usize) -> *mut c_void {
    mi_valloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn pvalloc(size: usize) -> *mut c_void {
    mi_pvalloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn cfree(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn reallocarray(p: *mut c_void, count: usize, size: usize) -> *mut c_void {
    mi_reallocarray(p, count, size)
}

#[no_mangle]
pub unsafe extern "C" fn reallocarr(p: *mut c_void, count: usize, size: usize) -> i32 {
    mi_reallocarr(p as *mut *mut c_void, count, size)
}

#[no_mangle]
pub unsafe extern "C" fn _aligned_malloc(size: usize, alignment: usize) -> *mut c_void {
    mi_malloc_aligned(size, alignment)
}

#[no_mangle]
pub unsafe extern "C" fn __libc_malloc(size: usize) -> *mut c_void {
    mi_malloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn __libc_calloc(count: usize, size: usize) -> *mut c_void {
    mi_calloc(count, size)
}

#[no_mangle]
pub unsafe extern "C" fn __libc_realloc(p: *mut c_void, size: usize) -> *mut c_void {
    mi_realloc(p, size)
}

#[no_mangle]
pub unsafe extern "C" fn __libc_free(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn __libc_cfree(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn __libc_valloc(size: usize) -> *mut c_void {
    mi_valloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn __libc_pvalloc(size: usize) -> *mut c_void {
    mi_pvalloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn __libc_memalign(alignment: usize, size: usize) -> *mut c_void {
    mi_memalign(alignment, size)
}

#[no_mangle]
pub unsafe extern "C" fn __posix_memalign(p: *mut *mut c_void, alignment: usize, size: usize) -> i32 {
    mi_posix_memalign(p, alignment, size)
}

// Itanium C++ new/delete (64-bit)
#[no_mangle]
pub unsafe extern "C" fn _ZdlPv(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPv(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdlPvm(p: *mut c_void, _n: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPvm(p: *mut c_void, _n: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _Znwm(n: usize) -> *mut c_void {
    mi_new(n)
}

#[no_mangle]
pub unsafe extern "C" fn _Znam(n: usize) -> *mut c_void {
    mi_new(n)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnwmRKSt9nothrow_t(n: usize, _tag: *const c_void) -> *mut c_void {
    mi_new_nothrow(n)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnamRKSt9nothrow_t(n: usize, _tag: *const c_void) -> *mut c_void {
    mi_new_nothrow(n)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnwmSt11align_val_t(n: usize, al: usize) -> *mut c_void {
    mi_new_aligned(n, al)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnamSt11align_val_t(n: usize, al: usize) -> *mut c_void {
    mi_new_aligned(n, al)
}

#[no_mangle]
pub unsafe extern "C" fn _ZdlPvSt11align_val_t(p: *mut c_void, _al: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPvSt11align_val_t(p: *mut c_void, _al: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdlPvmSt11align_val_t(p: *mut c_void, _n: usize, _al: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPvmSt11align_val_t(p: *mut c_void, _n: usize, _al: usize) {
    mi_free(p);
}

// GNU constructor so we init before most other libraries.
#[used]
#[link_section = ".init_array"]
static INIT: extern "C" fn() = mi_ctor;

extern "C" fn mi_ctor() {
    mimalloc_core::init();
}
