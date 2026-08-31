//! C ABI and libc malloc override (`cdylib` / `staticlib`).
//!
//! Thin `extern "C"` wrappers around [`mimalloc_core`]. Exports both `mi_*`
//! names (C `include/mimalloc.h`) and the standard allocator symbols so
//! `libmimalloc.so` / `libmimalloc-secure.so` works with `LD_PRELOAD` or as
//! NixOS `environment.memoryAllocator.provider = "mimalloc"`.
//!
//! `--features secure` sets SONAME `libmimalloc-secure.so.3` (C `-DMI_SECURE`).
//! Mitigations in the core are always on; the feature only changes the soname.
//! Install both SONAMEs so a program that `DT_NEEDED` the secure name (nixpkgs
//! mold) binds this rewrite instead of C mimalloc.
//!
//! C++ `operator new`/`delete` (`cxx_new_delete`) are strong `T` symbols,
//! matching C mimalloc's archive - not only `mimalloc-new-delete.h`.
//!
//! The glibc cdylib `DT_NEEDED`s `libc.so.6` and registers teardown with
//! `__cxa_atexit` (not `atexit`, which is `libc_nonshared` and shows up as
//! unversioned `U atexit`).
//!
//! # Invariants (ABI)
//!
//! - `malloc` / `mi_malloc` return 16-aligned pointers or null (`ENOMEM`).
//! - `free(NULL)` is a no-op; foreign pointers are ignored (not undefined).
//! - `mi_usable_size` is the user request size from the padding trailer.
//! - Heap/theap/arena/subproc pointers are the core types, `#[repr(C)]`.
//!
//! # Safety
//!
//! Every `mi_*` / libc export is `unsafe` C ABI. Pointers passed to `free` /
//! `realloc` must be from this allocator or null. Size/alignment arguments
//! follow POSIX and `include/mimalloc.h`.

#![cfg_attr(not(test), no_std)]
#![allow(non_snake_case)]

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};
use mimalloc_core::alloc as mi;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    mimalloc_core_abort()
}

/// Debug cdylibs still reference this even with `panic = abort`; LTO strips it in release.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn rust_eh_personality() {}

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

type OutputFun = unsafe extern "C" fn(*const libc::c_char, *mut c_void);

static OUT_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static OUT_ARG: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

unsafe fn emit_cstr(out: *mut c_void, arg: *mut c_void, msg: *const libc::c_char) {
    let (f, a) = if out.is_null() {
        (
            OUT_FN.load(Ordering::Acquire),
            OUT_ARG.load(Ordering::Acquire),
        )
    } else {
        (out as *mut (), arg)
    };
    if f.is_null() {
        let n = libc::strlen(msg);
        libc::write(2, msg as *const c_void, n);
    } else {
        let cb: OutputFun = core::mem::transmute(f);
        cb(msg, a);
    }
}

// ---------------------------------------------------------------------------
// mimalloc-prefixed API (`include/mimalloc.h`)
// Each function is a C ABI trampoline into `mimalloc_core`.
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
pub unsafe extern "C" fn mi_expand(p: *mut c_void, newsize: usize) -> *mut c_void {
    pvoid(mi::expand(pu8(p), newsize))
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
pub unsafe extern "C" fn mi_free_size(p: *mut c_void, size: usize) {
    mi::free_size(pu8(p), size);
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
pub unsafe extern "C" fn mi_process_done() {
    if mimalloc_core::mi_options::is_enabled(1) || mimalloc_core::mi_options::is_enabled(2) {
        // show_stats or verbose
        mi_stats_print_out(core::ptr::null_mut(), core::ptr::null_mut());
    }
}

#[no_mangle]
pub unsafe extern "C" fn mi_thread_init() {
    mimalloc_core::init();
}

#[no_mangle]
pub unsafe extern "C" fn mi_thread_done() {
    mimalloc_core::thread_done();
}

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
pub unsafe extern "C" fn mi_posix_memalign(
    p: *mut *mut c_void,
    alignment: usize,
    size: usize,
) -> i32 {
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

const TRY_NEW_MAX: i32 = 4;

type NewHandler = unsafe extern "C" fn();
type GetNewHandler = unsafe extern "C" fn() -> Option<NewHandler>;

unsafe fn cxx_get_new_handler() -> Option<NewHandler> {
    let p = libc::dlsym(
        libc::RTLD_DEFAULT,
        b"_ZSt15get_new_handlerv\0".as_ptr() as *const libc::c_char,
    );
    if p.is_null() {
        return None;
    }
    let getter: GetNewHandler = core::mem::transmute(p);
    getter()
}

unsafe fn try_new_handler(nothrow: bool) -> bool {
    match cxx_get_new_handler() {
        None => {
            if !nothrow {
                mimalloc_core_abort();
            }
            false
        }
        Some(h) => {
            h();
            true
        }
    }
}

unsafe fn try_malloc(size: usize, nothrow: bool) -> *mut c_void {
    let mut p = mi_malloc(size);
    if !p.is_null() {
        return p;
    }
    for _ in 0..TRY_NEW_MAX {
        if !try_new_handler(nothrow) {
            break;
        }
        p = mi_malloc(size);
        if !p.is_null() {
            return p;
        }
    }
    p
}

unsafe fn try_malloc_aligned(size: usize, alignment: usize, nothrow: bool) -> *mut c_void {
    let mut p = mi_malloc_aligned(size, alignment);
    if !p.is_null() {
        return p;
    }
    for _ in 0..TRY_NEW_MAX {
        if !try_new_handler(nothrow) {
            break;
        }
        p = mi_malloc_aligned(size, alignment);
        if !p.is_null() {
            return p;
        }
    }
    p
}

#[no_mangle]
pub unsafe extern "C" fn mi_new(size: usize) -> *mut c_void {
    let p = try_malloc(size, false);
    if p.is_null() {
        mimalloc_core_abort();
    }
    p
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_nothrow(size: usize) -> *mut c_void {
    try_malloc(size, true)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_aligned(size: usize, alignment: usize) -> *mut c_void {
    let p = try_malloc_aligned(size, alignment, false);
    if p.is_null() {
        mimalloc_core_abort();
    }
    p
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_aligned_nothrow(size: usize, alignment: usize) -> *mut c_void {
    try_malloc_aligned(size, alignment, true)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_n(count: usize, size: usize) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        try_new_handler(false);
        return core::ptr::null_mut();
    };
    mi_new(total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_alloc_new(
    heap: *mut mimalloc_core::Heap,
    size: usize,
) -> *mut c_void {
    let mut p = mi_heap_malloc(heap, size);
    if !p.is_null() {
        return p;
    }
    for _ in 0..TRY_NEW_MAX {
        if !try_new_handler(false) {
            break;
        }
        p = mi_heap_malloc(heap, size);
        if !p.is_null() {
            return p;
        }
    }
    if p.is_null() {
        mimalloc_core_abort();
    }
    p
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_alloc_new_n(
    heap: *mut mimalloc_core::Heap,
    count: usize,
    size: usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        try_new_handler(false);
        return core::ptr::null_mut();
    };
    mi_heap_alloc_new(heap, total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_realloc(p: *mut c_void, newsize: usize) -> *mut c_void {
    let mut q = mi_realloc(p, newsize);
    if !q.is_null() || (p.is_null() && newsize == 0) {
        return q;
    }
    for _ in 0..TRY_NEW_MAX {
        if !try_new_handler(false) {
            break;
        }
        q = mi_realloc(p, newsize);
        if !q.is_null() {
            return q;
        }
    }
    if q.is_null() {
        mimalloc_core_abort();
    }
    q
}

#[no_mangle]
pub unsafe extern "C" fn mi_new_reallocn(p: *mut c_void, count: usize, size: usize) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        try_new_handler(false);
        return core::ptr::null_mut();
    };
    mi_new_realloc(p, total)
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
pub unsafe extern "C" fn mi_rezalloc_aligned_at(
    p: *mut c_void,
    newsize: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    pvoid(mi::rezalloc_aligned_at(pu8(p), newsize, alignment, offset))
}

#[no_mangle]
pub unsafe extern "C" fn mi_recalloc_aligned(
    p: *mut c_void,
    newcount: usize,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    let Some(total) = newcount.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_rezalloc_aligned(p, total, alignment)
}

#[no_mangle]
pub unsafe extern "C" fn mi_recalloc_aligned_at(
    p: *mut c_void,
    newcount: usize,
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    let Some(total) = newcount.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_rezalloc_aligned_at(p, total, alignment, offset)
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
pub unsafe extern "C" fn mi_heap_malloc(
    heap: *mut mimalloc_core::Heap,
    size: usize,
) -> *mut c_void {
    pvoid(mimalloc_core::heap_malloc(heap, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_zalloc(
    heap: *mut mimalloc_core::Heap,
    size: usize,
) -> *mut c_void {
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
pub unsafe extern "C" fn mi_heap_malloc_aligned_at(
    heap: *mut mimalloc_core::Heap,
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    pvoid(mimalloc_core::heap_malloc_aligned_at(
        heap, size, alignment, offset,
    ))
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
pub unsafe extern "C" fn mi_heap_main() -> *mut mimalloc_core::Heap {
    mimalloc_core::heap_main()
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_of(p: *const c_void) -> *mut mimalloc_core::Heap {
    mimalloc_core::heap_of(p as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_contains(
    heap: *const mimalloc_core::Heap,
    p: *const c_void,
) -> bool {
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
pub unsafe extern "C" fn mi_heap_set_numa_affinity(heap: *mut mimalloc_core::Heap, numa_node: i32) {
    mimalloc_core::heap_set_numa_affinity(heap, numa_node);
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_theap(
    heap: *mut mimalloc_core::Heap,
) -> *mut mimalloc_core::Theap {
    mimalloc_core::heap_theap(heap)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_get_default() -> *mut mimalloc_core::Theap {
    mimalloc_core::theap_get_default()
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_set_default(
    theap: *mut mimalloc_core::Theap,
) -> *mut mimalloc_core::Theap {
    mimalloc_core::theap_set_default(theap)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_collect(theap: *mut mimalloc_core::Theap, force: bool) {
    mimalloc_core::theap_collect(theap, force);
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_malloc(
    theap: *mut mimalloc_core::Theap,
    size: usize,
) -> *mut c_void {
    pvoid(mimalloc_core::theap_malloc(theap, size))
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_zalloc(
    theap: *mut mimalloc_core::Theap,
    size: usize,
) -> *mut c_void {
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
    theap: *mut mimalloc_core::Theap,
    sample_rate: usize,
    seed: usize,
) {
    mimalloc_core::theap_guarded_set_sample_rate(theap, sample_rate, seed);
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_guarded_set_size_bound(
    theap: *mut mimalloc_core::Theap,
    min: usize,
    max: usize,
) {
    mimalloc_core::theap_guarded_set_size_bound(theap, min, max);
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
    mimalloc_core::theap_visit_blocks(
        theap as *mut mimalloc_core::Theap,
        visit_blocks,
        visitor,
        arg,
    )
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
    mi_reserve_huge_os_pages_at_ex(
        pages,
        _numa_node,
        _timeout_msecs,
        false,
        core::ptr::null_mut(),
    )
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
pub unsafe extern "C" fn mi_register_deferred_free(f: *mut c_void, arg: *mut c_void) {
    mimalloc_core::hooks::register_deferred_free(f, arg);
}

#[no_mangle]
pub unsafe extern "C" fn mi_register_output(f: *mut c_void, arg: *mut c_void) {
    OUT_FN.store(f as *mut (), Ordering::Release);
    OUT_ARG.store(arg, Ordering::Release);
}

#[no_mangle]
pub unsafe extern "C" fn mi_register_error(f: *mut c_void, arg: *mut c_void) {
    mimalloc_core::hooks::register_error(f, arg);
}

#[no_mangle]
pub unsafe extern "C" fn mi_thread_set_in_threadpool() {
    mimalloc_core::theap_set_in_threadpool(mimalloc_core::theap_get_default());
}

#[no_mangle]
pub unsafe extern "C" fn mi_collect_reduce(target_thread_owned: usize) {
    mimalloc_core::collect_reduce(target_thread_owned);
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
pub unsafe extern "C" fn mi_process_info_print() {
    mi_process_info_print_out(core::ptr::null_mut(), core::ptr::null_mut());
}

#[no_mangle]
pub unsafe extern "C" fn mi_process_info_print_out(out: *mut c_void, arg: *mut c_void) {
    let mut elapsed = 0usize;
    let mut user = 0usize;
    let mut sys = 0usize;
    let mut rss = 0usize;
    let mut peak_rss = 0usize;
    let mut commit = 0usize;
    let mut peak_commit = 0usize;
    let mut faults = 0usize;
    mi_process_info(
        &mut elapsed,
        &mut user,
        &mut sys,
        &mut rss,
        &mut peak_rss,
        &mut commit,
        &mut peak_commit,
        &mut faults,
    );
    let mut buf = [0 as libc::c_char; 256];
    libc::snprintf(
        buf.as_mut_ptr(),
        buf.len(),
        b"elapsed: %zu ms, user: %zu ms, sys: %zu ms, rss: %zu, peak rss: %zu, page faults: %zu\n\0"
            .as_ptr() as *const libc::c_char,
        elapsed,
        user,
        sys,
        rss,
        peak_rss,
        faults,
    );
    emit_cstr(out, arg, buf.as_ptr());
}

#[no_mangle]
pub unsafe extern "C" fn mi_options_print_out(out: *mut c_void, arg: *mut c_void) {
    let mut buf = [0 as libc::c_char; 128];
    libc::snprintf(
        buf.as_mut_ptr(),
        buf.len(),
        b"v%i.%i.%i (rust rewrite)\n\0".as_ptr() as *const libc::c_char,
        mimalloc_core::MI_MALLOC_VERSION / 10000,
        (mimalloc_core::MI_MALLOC_VERSION % 10000) / 100,
        mimalloc_core::MI_MALLOC_VERSION % 100,
    );
    emit_cstr(out, arg, buf.as_ptr());
    for i in 0..mimalloc_core::mi_options::NAMES.len() {
        let Some(name) = mimalloc_core::mi_options::name(i as i32) else {
            continue;
        };
        let mut namebuf = [0u8; 64];
        if name.len() + 1 > namebuf.len() {
            continue;
        }
        namebuf[..name.len()].copy_from_slice(name.as_bytes());
        libc::snprintf(
            buf.as_mut_ptr(),
            buf.len(),
            b"option '%s': %ld\n\0".as_ptr() as *const libc::c_char,
            namebuf.as_ptr(),
            mimalloc_core::mi_options::get(i as i32) as libc::c_long,
        );
        emit_cstr(out, arg, buf.as_ptr());
    }
}

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
pub unsafe extern "C" fn mi_heap_strndup(
    heap: *mut mimalloc_core::Heap,
    s: *const libc::c_char,
    n: usize,
) -> *mut libc::c_char {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    let mut len = 0usize;
    while len < n && *s.add(len) != 0 {
        len += 1;
    }
    let d = mimalloc_core::heap_malloc(heap, len + 1) as *mut libc::c_char;
    if !d.is_null() {
        core::ptr::copy_nonoverlapping(s, d, len);
        *d.add(len) = 0;
    }
    d
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_realpath(
    heap: *mut mimalloc_core::Heap,
    fname: *const libc::c_char,
    resolved_name: *mut libc::c_char,
) -> *mut libc::c_char {
    if !resolved_name.is_null() {
        return libc::realpath(fname, resolved_name);
    }
    const PATH_MAX: usize = 4096;
    let mut buf = [0 as libc::c_char; PATH_MAX];
    let r = libc::realpath(fname, buf.as_mut_ptr());
    if r.is_null() {
        return core::ptr::null_mut();
    }
    mi_heap_strdup(heap, buf.as_ptr())
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
pub unsafe extern "C" fn mi_heap_zalloc_aligned_at(
    heap: *mut mimalloc_core::Heap,
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    let p = mimalloc_core::heap_malloc_aligned_at(heap, size, alignment, offset);
    if !p.is_null() {
        core::ptr::write_bytes(p, 0, size);
    }
    pvoid(p)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_calloc_aligned(
    heap: *mut mimalloc_core::Heap,
    count: usize,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_heap_zalloc_aligned(heap, total, alignment)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_calloc_aligned_at(
    heap: *mut mimalloc_core::Heap,
    count: usize,
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_heap_zalloc_aligned_at(heap, total, alignment, offset)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_realloc_aligned(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newsize: usize,
    alignment: usize,
) -> *mut c_void {
    mi_heap_realloc_aligned_at(heap, p, newsize, alignment, 0)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_realloc_aligned_at(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newsize: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    if p.is_null() {
        return mi_heap_malloc_aligned_at(heap, newsize, alignment, offset);
    }
    let old = mi::usable_size(p as *const u8);
    let aligned_ok = if offset % alignment == 0 {
        (p as usize) % alignment == 0
    } else {
        (p as usize).wrapping_add(offset) % alignment == 0
    };
    if old >= newsize && aligned_ok {
        return p;
    }
    let q = mimalloc_core::heap_malloc_aligned_at(heap, newsize, alignment, offset);
    if q.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(p as *const u8, q, old.min(newsize));
    mi::free(pu8(p));
    pvoid(q)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_recalloc(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newcount: usize,
    size: usize,
) -> *mut c_void {
    let Some(total) = newcount.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_heap_rezalloc(heap, p, total)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_rezalloc_aligned(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newsize: usize,
    alignment: usize,
) -> *mut c_void {
    mi_heap_rezalloc_aligned_at(heap, p, newsize, alignment, 0)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_rezalloc_aligned_at(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newsize: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    if p.is_null() {
        return mi_heap_zalloc_aligned_at(heap, newsize, alignment, offset);
    }
    let old = mi::usable_size(p as *const u8);
    let q = mi_heap_realloc_aligned_at(heap, p, newsize, alignment, offset);
    if !q.is_null() && newsize > old {
        core::ptr::write_bytes((q as *mut u8).add(old), 0, newsize - old);
    }
    q
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_recalloc_aligned(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newcount: usize,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    let Some(total) = newcount.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_heap_rezalloc_aligned(heap, p, total, alignment)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_recalloc_aligned_at(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    newcount: usize,
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    let Some(total) = newcount.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    mi_heap_rezalloc_aligned_at(heap, p, total, alignment, offset)
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
pub unsafe extern "C" fn mi_free_aligned(p: *mut c_void, _alignment: usize) {
    mi_free(p);
}

unsafe fn print_stats(stats: &mimalloc_core::Stats, out: *mut c_void, arg: *mut c_void) {
    let mut buf = [0 as libc::c_char; 256];
    libc::snprintf(
        buf.as_mut_ptr(),
        buf.len(),
        b"mimalloc: pages current %lld peak %lld total %lld, malloc current %lld peak %lld count %lld\n\0"
            .as_ptr() as *const libc::c_char,
        stats.pages.current as libc::c_longlong,
        stats.pages.peak as libc::c_longlong,
        stats.pages.total as libc::c_longlong,
        stats.malloc_requested.current as libc::c_longlong,
        stats.malloc_requested.peak as libc::c_longlong,
        stats.malloc_normal_count.total as libc::c_longlong,
    );
    emit_cstr(out, arg, buf.as_ptr());
}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_print(out: *mut c_void) {
    mi_stats_print_out(out, core::ptr::null_mut());
}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_print_out(out: *mut c_void, arg: *mut c_void) {
    let mut stats = core::mem::zeroed();
    if !mi_stats_get(&mut stats) {
        return;
    }
    print_stats(&stats, out, arg);
}

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
pub unsafe extern "C" fn mi_options_print() {
    mi_options_print_out(core::ptr::null_mut(), core::ptr::null_mut());
}

#[repr(C)]
struct ArenaPrintCtx {
    out: *mut c_void,
    arg: *mut c_void,
    n: usize,
}

unsafe extern "C" fn arena_print_visitor(a: *mut mimalloc_core::Arena, arg: *mut c_void) -> bool {
    let ctx = &mut *(arg as *mut ArenaPrintCtx);
    let exclusive = if (*a).exclusive {
        b", exclusive\0".as_ptr()
    } else {
        b"\0".as_ptr()
    };
    let owned = if (*a).owned {
        b", owned\0".as_ptr()
    } else {
        b"\0".as_ptr()
    };
    let mut buf = [0 as libc::c_char; 256];
    libc::snprintf(
        buf.as_mut_ptr(),
        buf.len(),
        b"arena %zu at %p: %zu bytes%s%s, numa: %i\n\0".as_ptr() as *const libc::c_char,
        ctx.n,
        a,
        (*a).size,
        exclusive as *const libc::c_char,
        owned as *const libc::c_char,
        (*a).numa_node,
    );
    emit_cstr(ctx.out, ctx.arg, buf.as_ptr());
    ctx.n += 1;
    true
}

#[no_mangle]
pub unsafe extern "C" fn mi_debug_show_arenas() {
    mi_arenas_print();
}

#[no_mangle]
pub unsafe extern "C" fn mi_arenas_print() {
    let mut ctx = ArenaPrintCtx {
        out: core::ptr::null_mut(),
        arg: core::ptr::null_mut(),
        n: 0,
    };
    mimalloc_core::mi_arena::visit_all(
        arena_print_visitor,
        &mut ctx as *mut ArenaPrintCtx as *mut c_void,
    );
}

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

#[no_mangle]
pub unsafe extern "C" fn mi_option_set_enabled_default(option: i32, enable: bool) {
    mi_option_set_enabled(option, enable);
}

#[no_mangle]
pub unsafe extern "C" fn mi__expand(p: *mut c_void, newsize: usize) -> *mut c_void {
    let q = mi_expand(p, newsize);
    if q.is_null() {
        *libc::__errno_location() = libc::ENOMEM;
    }
    q
}

#[no_mangle]
pub unsafe extern "C" fn mi_aligned_recalloc(
    p: *mut c_void,
    newcount: usize,
    size: usize,
    alignment: usize,
) -> *mut c_void {
    mi_recalloc_aligned(p, newcount, size, alignment)
}

#[no_mangle]
pub unsafe extern "C" fn mi_aligned_offset_recalloc(
    p: *mut c_void,
    newcount: usize,
    size: usize,
    alignment: usize,
    offset: usize,
) -> *mut c_void {
    mi_recalloc_aligned_at(p, newcount, size, alignment, offset)
}

#[no_mangle]
pub unsafe extern "C" fn mi_free_size_aligned(p: *mut c_void, size: usize, alignment: usize) {
    mi::free_size_aligned(pu8(p), size, alignment);
}

#[no_mangle]
pub unsafe extern "C" fn mi_mbsdup(s: *const u8) -> *mut u8 {
    mi_strdup(s as *const libc::c_char) as *mut u8
}

#[no_mangle]
pub unsafe extern "C" fn mi_wcsdup(s: *const libc::wchar_t) -> *mut libc::wchar_t {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    let mut n = 0usize;
    while *s.add(n) != 0 {
        n += 1;
    }
    let bytes = (n + 1).saturating_mul(core::mem::size_of::<libc::wchar_t>());
    let p = mi::malloc(bytes) as *mut libc::wchar_t;
    if !p.is_null() {
        core::ptr::copy_nonoverlapping(s, p, n + 1);
    }
    p
}

#[no_mangle]
pub unsafe extern "C" fn mi_dupenv_s(
    buf: *mut *mut libc::c_char,
    size: *mut usize,
    name: *const libc::c_char,
) -> i32 {
    if !size.is_null() {
        *size = 0;
    }
    if buf.is_null() || name.is_null() {
        return libc::EINVAL;
    }
    let p = libc::getenv(name);
    if p.is_null() {
        *buf = core::ptr::null_mut();
        return 0;
    }
    *buf = mi_strdup(p);
    if (*buf).is_null() {
        return libc::ENOMEM;
    }
    if !size.is_null() {
        *size = libc::strlen(p) + 1;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn mi_wdupenv_s(
    buf: *mut *mut libc::wchar_t,
    size: *mut usize,
    name: *const libc::wchar_t,
) -> i32 {
    if !size.is_null() {
        *size = 0;
    }
    if buf.is_null() || name.is_null() {
        return libc::EINVAL;
    }
    *buf = core::ptr::null_mut();
    libc::EINVAL
}

#[no_mangle]
pub unsafe extern "C" fn mi_manage_memory(
    start: *mut c_void,
    size: usize,
    is_committed: bool,
    is_pinned: bool,
    is_zero: bool,
    numa_node: i32,
    exclusive: bool,
    _commit_fun: *mut c_void,
    _commit_fun_arg: *mut c_void,
    arena_id: *mut *mut c_void,
) -> bool {
    mi_manage_os_memory_ex(
        start,
        size,
        is_committed,
        is_pinned,
        is_zero,
        numa_node,
        exclusive,
        arena_id,
    )
}

#[no_mangle]
pub unsafe extern "C" fn mi_unsafe_heap_page_is_under_utilized(
    heap: *mut mimalloc_core::Heap,
    p: *mut c_void,
    perc_threshold: usize,
) -> bool {
    mimalloc_core::page_is_under_utilized(heap, p as *const u8, perc_threshold)
}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_merge() {
    mimalloc_core::stats_merge();
}

#[no_mangle]
pub unsafe extern "C" fn mi_thread_stats_print_out(out: *mut c_void, arg: *mut c_void) {
    let mut stats = core::mem::zeroed();
    if !mi_theap_stats_get(mi_theap_get_default(), &mut stats) {
        return;
    }
    print_stats(&stats, out, arg);
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_stats_merge_to_subproc(heap: *mut mimalloc_core::Heap) {
    mimalloc_core::heap_stats_merge_to_subproc(heap);
}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_get_bin_size(bin: usize) -> usize {
    mimalloc_core::mi_stats::get_bin_size(bin)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_stats_get(
    heap: *mut mimalloc_core::Heap,
    stats: *mut mimalloc_core::Stats,
) -> bool {
    mimalloc_core::heap_stats_get(heap, stats)
}

#[no_mangle]
pub unsafe extern "C" fn mi_theap_stats_get(
    theap: *mut mimalloc_core::Theap,
    stats: *mut mimalloc_core::Stats,
) -> bool {
    mimalloc_core::theap_stats_get(theap, stats)
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_stats_get(
    subproc: mimalloc_core::SubprocId,
    stats: *mut mimalloc_core::Stats,
) -> bool {
    mimalloc_core::mi_subproc::stats_get(subproc, stats, false)
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_stats_get_exclusive(
    subproc: mimalloc_core::SubprocId,
    stats: *mut mimalloc_core::Stats,
) -> bool {
    mimalloc_core::mi_subproc::stats_get(subproc, stats, true)
}

unsafe fn stats_as_json(
    stats: *mut mimalloc_core::Stats,
    buf_size: usize,
    buf: *mut libc::c_char,
) -> *mut libc::c_char {
    if stats.is_null() {
        return core::ptr::null_mut();
    }
    let mut tmp = [0 as libc::c_char; 256];
    let n = libc::snprintf(
        tmp.as_mut_ptr(),
        tmp.len(),
        b"{\"stat_version\":%zu,\"mimalloc_version\":%d,\"pages\":{\"current\":%lld,\"peak\":%lld,\"total\":%lld}}\n\0"
            .as_ptr() as *const libc::c_char,
        (*stats).version,
        mimalloc_core::MI_MALLOC_VERSION,
        (*stats).pages.current as libc::c_longlong,
        (*stats).pages.peak as libc::c_longlong,
        (*stats).pages.total as libc::c_longlong,
    );
    if n < 0 {
        return core::ptr::null_mut();
    }
    let need = (n as usize) + 1;
    if buf.is_null() {
        let out = mi::malloc(need) as *mut libc::c_char;
        if out.is_null() {
            return core::ptr::null_mut();
        }
        core::ptr::copy_nonoverlapping(tmp.as_ptr(), out, need);
        return out;
    }
    if buf_size < need {
        return core::ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(tmp.as_ptr(), buf, need);
    buf
}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_as_json(
    stats: *mut mimalloc_core::Stats,
    buf_size: usize,
    buf: *mut libc::c_char,
) -> *mut libc::c_char {
    stats_as_json(stats, buf_size, buf)
}

#[no_mangle]
pub unsafe extern "C" fn mi_stats_get_json(
    buf_size: usize,
    buf: *mut libc::c_char,
) -> *mut libc::c_char {
    let mut stats = core::mem::zeroed();
    if !mi_stats_get(&mut stats) {
        return core::ptr::null_mut();
    }
    stats_as_json(&mut stats, buf_size, buf)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_stats_get_json(
    heap: *mut mimalloc_core::Heap,
    buf_size: usize,
    buf: *mut libc::c_char,
) -> *mut libc::c_char {
    let mut stats = core::mem::zeroed();
    if !mi_heap_stats_get(heap, &mut stats) {
        return core::ptr::null_mut();
    }
    stats_as_json(&mut stats, buf_size, buf)
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_stats_get_json(
    subproc: mimalloc_core::SubprocId,
    buf_size: usize,
    buf: *mut libc::c_char,
) -> *mut libc::c_char {
    let mut stats = core::mem::zeroed();
    if !mi_subproc_stats_get(subproc, &mut stats) {
        return core::ptr::null_mut();
    }
    stats_as_json(&mut stats, buf_size, buf)
}

#[no_mangle]
pub unsafe extern "C" fn mi_heap_stats_print_out(
    _heap: *mut mimalloc_core::Heap,
    out: *mut c_void,
    arg: *mut c_void,
) {
    mi_stats_print_out(out, arg);
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_stats_print_out(
    _subproc: mimalloc_core::SubprocId,
    out: *mut c_void,
    arg: *mut c_void,
) {
    mi_stats_print_out(out, arg);
}

#[no_mangle]
pub unsafe extern "C" fn mi_subproc_heap_stats_print_out(
    _subproc: mimalloc_core::SubprocId,
    out: *mut c_void,
    arg: *mut c_void,
) {
    mi_stats_print_out(out, arg);
}

// ---------------------------------------------------------------------------
// libc override symbols (`malloc`, `free`, `posix_memalign`, …)
// Must be strong `T` so `LD_PRELOAD` interposes glibc/musl.
//
// Do not export `strdup` / `reallocarray` / `__libc_*`. Chromium's executable
// owns `realloc` (PartitionAlloc). DSOs still bind those extra symbols to a
// preloaded allocator, then PA `realloc`s the pointers and SIGSEGVs. Graphene
// hardened_malloc-light stays compatible by exporting only the malloc family.
// `mi_strdup` / `mi_reallocarray` remain on the C API.
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
pub unsafe extern "C" fn _aligned_malloc(size: usize, alignment: usize) -> *mut c_void {
    mi_malloc_aligned(size, alignment)
}

// Itanium C++ new/delete (64-bit). Same strong symbols as C mimalloc's
// shared/static libraries. Programs that `#include <mimalloc-new-delete.h>`
// must not also statically whole-archive this library (mold does that
// include only for a *dynamic* system mimalloc).
#[path = "cxx_new_delete.rs"]
mod cxx_new_delete;

// GNU constructor so we init before most other libraries.
#[used]
#[link_section = ".init_array"]
static INIT: extern "C" fn() = mi_ctor;

#[cfg(not(target_env = "gnu"))]
extern "C" fn process_done_atexit() {
    unsafe {
        mi_process_done();
    }
}

#[cfg(target_env = "gnu")]
unsafe extern "C" fn process_done_cxa(_: *mut c_void) {
    mi_process_done();
}

extern "C" fn mi_ctor() {
    mimalloc_core::init();
    // glibc's `atexit` lives in libc_nonshared.a. A cdylib linked without the
    // gcc driver (nixpkgs rustc/lld) leaves an unversioned `U atexit` that
    // is not in libc.so.6, so LD_PRELOAD fails with "undefined symbol: atexit".
    #[cfg(target_env = "gnu")]
    {
        unsafe extern "C" {
            fn __cxa_atexit(
                f: Option<unsafe extern "C" fn(*mut c_void)>,
                arg: *mut c_void,
                dso: *mut c_void,
            ) -> i32;
        }
        let _ = unsafe {
            __cxa_atexit(
                Some(process_done_cxa),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
    }
    #[cfg(not(target_env = "gnu"))]
    {
        let _ = unsafe { libc::atexit(process_done_atexit) };
    }
}
