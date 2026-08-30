//! Pure-Rust mimalloc-inspired allocator core (`no_std`, no `alloc` crate).

#![no_std]
#![allow(clippy::missing_safety_doc)]

pub mod alloc;
pub mod arena;
mod bin;
pub mod global;
mod heap;
pub mod hooks;
pub mod options;
mod os;
mod page;
mod page_map;
mod spin;
pub mod stats;
pub mod subproc;
mod tls;

use core::sync::atomic::{AtomicBool, Ordering};

pub const PTR_SIZE: usize = core::mem::size_of::<usize>();
pub const SLICE_SHIFT: usize = 16;
pub const SLICE_SIZE: usize = 1 << SLICE_SHIFT;
pub const MEDIUM_PAGE_SIZE: usize = 8 * SLICE_SIZE;
pub const LARGE_PAGE_SIZE: usize = PTR_SIZE * MEDIUM_PAGE_SIZE;
pub const LARGE_MAX_OBJ_SIZE: usize = LARGE_PAGE_SIZE / 8;
pub const BIN_HUGE: usize = 73;
pub const MAX_ALLOC: usize = isize::MAX as usize;
/// 8-byte `{canary, delta}` trailer at the end of every block (C `MI_PADDING`).
pub const PADDING_SIZE: usize = 8;
pub const MI_MALLOC_VERSION: i32 = 30500;

static INIT_DONE: AtomicBool = AtomicBool::new(false);
static INIT_LOCK: spin::SpinLock = spin::SpinLock::new();

#[inline]
pub fn is_init_done() -> bool {
    INIT_DONE.load(Ordering::Acquire)
}

#[inline]
pub fn align_up(x: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (x + align - 1) & !(align - 1)
}

pub fn init() {
    if INIT_DONE.load(Ordering::Acquire) {
        return;
    }
    let _g = INIT_LOCK.lock();
    if INIT_DONE.load(Ordering::Acquire) {
        return;
    }
    tls::IN_BOOTSTRAP.store(true, Ordering::Release);
    tls::BOOTSTRAP_TID.store(os::gettid(), Ordering::Release);
    os::init();
    bin::init_bin_sizes();
    page_map::init();
    options::init();
    tls::register_atfork();
    tls::BOOTSTRAP_TID.store(0, Ordering::Release);
    tls::IN_BOOTSTRAP.store(false, Ordering::Release);
    INIT_DONE.store(true, Ordering::Release);
}

/// Child after `fork`: locks may have been held by threads that do not exist here.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) unsafe fn fork_child() {
    INIT_LOCK.force_unlock();
    tls::force_unlock();
    heap::force_unlock_all();
}

pub use alloc::{
    aligned_alloc, calloc, collect, collect_reduce, expand, free, free_size, free_size_aligned,
    good_size, malloc, malloc_aligned, malloc_aligned_at, manage_os_memory_ex, memalign,
    posix_memalign, pvalloc, realloc, reallocarr, reallocarray, reallocf, realpath,
    reserve_os_memory, reserve_os_memory_ex, rezalloc, rezalloc_aligned, rezalloc_aligned_at,
    strdup, strndup, ufree, umalloc, urealloc, usable_size, valloc, VERSION,
};
pub use arena::{self as mi_arena, Arena};
pub use heap::{
    any_heap_contains, collect_all, heap_collect, heap_contains, heap_delete, heap_destroy,
    heap_main, heap_malloc, heap_malloc_aligned, heap_malloc_aligned_at, heap_new,
    heap_new_in_arena, heap_numa_node, heap_of, heap_set_numa_affinity, heap_stats_get,
    heap_stats_merge_to_subproc, heap_theap, heap_visit_abandoned_blocks, heap_visit_blocks,
    page_is_under_utilized, stats_merge, theap_collect, theap_get_default,
    theap_guarded_set_sample_rate, theap_guarded_set_size_bound, theap_malloc,
    theap_malloc_aligned, theap_malloc_aligned_at, theap_set_default, theap_set_in_threadpool,
    theap_stats_get, theap_visit_blocks, BlockVisitFun, Heap, HeapArea, Theap,
};
pub use options as mi_options;
pub use stats::{self as mi_stats, Stats};
pub use subproc::{self as mi_subproc, Subproc, SubprocId};
pub use global::Mimalloc;
pub use tls::thread_done;

#[cfg(test)]
mod tests {
    extern crate std;
    use super::alloc;
    use std::vec::Vec;

    #[test]
    fn malloc_write_free() {
        unsafe {
            let p = alloc::malloc(64);
            assert!(!p.is_null());
            core::ptr::write_bytes(p, 0xAB, 64);
            assert_eq!(*p, 0xAB);
            alloc::free(p);
        }
    }

    #[test]
    fn global_alloc_trait() {
        use core::alloc::{GlobalAlloc, Layout};
        let a = crate::Mimalloc;
        let layout = Layout::from_size_align(48, 8).unwrap();
        unsafe {
            let p = a.alloc(layout);
            assert!(!p.is_null());
            core::ptr::write_bytes(p, 0xCD, 48);
            assert_eq!(*p, 0xCD);
            let q = a.realloc(p, layout, 96);
            assert!(!q.is_null());
            assert_eq!(*q, 0xCD);
            a.dealloc(q, Layout::from_size_align(96, 8).unwrap());
        }
    }

    #[test]
    fn calloc_is_zero() {
        unsafe {
            let p = alloc::calloc(8, 16);
            assert!(!p.is_null());
            for i in 0..128 {
                assert_eq!(*p.add(i), 0);
            }
            alloc::free(p);
        }
    }

    #[test]
    fn realloc_grows_and_preserves() {
        unsafe {
            let p = alloc::malloc(8);
            core::ptr::write_bytes(p, 0x11, 8);
            let q = alloc::realloc(p, 4096);
            assert!(!q.is_null());
            assert_eq!(*q, 0x11);
            alloc::free(q);
        }
    }

    #[test]
    fn aligned_allocs() {
        unsafe {
            let p = alloc::malloc_aligned(64, 64);
            assert!(!p.is_null());
            assert_eq!(p as usize % 64, 0);
            alloc::free(p);
            let mut q: *mut u8 = core::ptr::null_mut();
            assert_eq!(alloc::posix_memalign(&mut q, 128, 32), 0);
            assert_eq!(q as usize % 128, 0);
            alloc::free(q);
            // Non-power-of-two size class that is a multiple of the alignment.
            for _ in 0..10 {
                let r = alloc::malloc_aligned(96, 32);
                assert!(!r.is_null());
                assert_eq!(r as usize % 32, 0);
                alloc::free(r);
            }
            let s = alloc::malloc_aligned(8, 8192);
            assert!(!s.is_null());
            assert_eq!(s as usize % 8192, 0);
            alloc::free(s);
            let z = alloc::malloc_aligned(0, 16);
            assert!(!z.is_null());
            assert_eq!(z as usize % 16, 0);
            alloc::free(z);
            let t = alloc::malloc_aligned(24, 16);
            assert!(!t.is_null());
            assert_eq!(t as usize % 16, 0);
            alloc::free(t);
        }
    }

    #[test]
    fn posix_memalign_errors_leave_out_unchanged() {
        unsafe {
            let sentinel = 0x1111 as *mut u8;
            let mut p = sentinel;
            assert_eq!(alloc::posix_memalign(&mut p, 3, 32), crate::os::EINVAL);
            assert_eq!(p, sentinel);
            p = sentinel;
            assert_eq!(
                alloc::posix_memalign(&mut p, 3 * crate::PTR_SIZE, 32),
                crate::os::EINVAL
            );
            assert_eq!(p, sentinel);
            p = sentinel;
            assert_eq!(
                alloc::posix_memalign(&mut p, crate::PTR_SIZE, usize::MAX),
                crate::os::ENOMEM
            );
            assert_eq!(p, sentinel);
        }
    }

    #[test]
    fn huge_alloc() {
        unsafe {
            let n = 2 * 1024 * 1024;
            let p = alloc::malloc(n);
            assert!(!p.is_null());
            *p = 7;
            *p.add(n - 1) = 9;
            assert_eq!(alloc::usable_size(p), n);
            alloc::free(p);
        }
    }

    #[test]
    fn many_small() {
        unsafe {
            let mut v: Vec<*mut u8> = Vec::new();
            for i in 0..10_000 {
                let p = alloc::malloc((i % 128) + 1);
                assert!(!p.is_null());
                *p = (i % 251) as u8;
                v.push(p);
            }
            for (i, p) in v.iter().enumerate() {
                assert_eq!(**p, (i % 251) as u8);
                alloc::free(*p);
            }
        }
    }

    #[test]
    fn free_null_is_ok() {
        unsafe {
            alloc::free(core::ptr::null_mut());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_allocs() {
        extern crate std;
        use std::thread;
        let mut handles = std::vec::Vec::new();
        for t in 0..8 {
            handles.push(thread::spawn(move || unsafe {
                let mut ptrs = std::vec::Vec::new();
                for i in 0..2000 {
                    let n = (i + t) % 256 + 1;
                    let p = alloc::malloc(n);
                    assert!(!p.is_null());
                    *p = i as u8;
                    ptrs.push(p);
                }
                for (i, p) in ptrs.iter().enumerate() {
                    assert_eq!(**p, i as u8);
                    alloc::free(*p);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn heap_new_malloc_destroy() {
        unsafe {
            let h = crate::heap_new();
            assert!(!h.is_null());
            let p = crate::heap_malloc(h, 64);
            assert!(!p.is_null());
            *p = 42;
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn heap_delete_then_free() {
        unsafe {
            let h = crate::heap_new();
            let p = crate::heap_malloc(h, 32);
            assert!(!p.is_null());
            crate::heap_delete(h);
            *p = 7;
            alloc::free(p);
        }
    }

    #[test]
    fn strdup_roundtrip() {
        unsafe {
            let s = b"hello mimalloc\0".as_ptr() as *const core::ffi::c_char;
            let d = alloc::strdup(s);
            assert!(!d.is_null());
            let mut i = 0;
            while *s.add(i) != 0 {
                assert_eq!(*d.add(i), *s.add(i));
                i += 1;
            }
            alloc::free(d as *mut u8);
        }
    }

    #[test]
    fn theap_malloc_and_contains() {
        unsafe {
            let h = crate::heap_new();
            let t = crate::heap_theap(h);
            assert!(!t.is_null());
            let p = crate::theap_malloc(t, 48);
            assert!(!p.is_null());
            *p = 9;
            assert!(crate::heap_contains(h, p as *const u8));
            assert_eq!(crate::heap_of(p as *const u8), h);
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn theap_set_default_routes_malloc() {
        unsafe {
            let h = crate::heap_new();
            let t = crate::heap_theap(h);
            let prev = crate::theap_set_default(t);
            let p = alloc::malloc(24);
            assert!(!p.is_null());
            assert!(crate::heap_contains(h, p as *const u8));
            alloc::free(p);
            crate::theap_set_default(prev);
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn arena_exclusive_heap() {
        unsafe {
            let mut id: *mut crate::Arena = core::ptr::null_mut();
            assert_eq!(
                alloc::reserve_os_memory_ex(2 * 1024 * 1024, true, false, true, &mut id),
                0
            );
            assert!(!id.is_null());
            let h = crate::heap_new_in_arena(id);
            assert!(!h.is_null());
            let p = crate::heap_malloc(h, 64);
            assert!(!p.is_null());
            assert!(crate::mi_arena::contains(id, p as *const u8));
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn heap_main_is_stable() {
        unsafe {
            let a = crate::heap_main();
            let b = crate::heap_main();
            assert!(!a.is_null());
            assert_eq!(a, b);
            let p = crate::heap_malloc(a, 16);
            assert!(!p.is_null());
            assert!(crate::heap_contains(a, p as *const u8));
            alloc::free(p);
        }
    }

    unsafe extern "C" fn count_visit(
        _heap: *const crate::Heap,
        _area: *const crate::HeapArea,
        block: *mut core::ffi::c_void,
        _bs: usize,
        arg: *mut core::ffi::c_void,
    ) -> bool {
        if !block.is_null() {
            let n = arg as *mut usize;
            *n += 1;
        }
        true
    }

    #[test]
    fn heap_visit_counts_live_blocks() {
        unsafe {
            let h = crate::heap_new();
            let mut ptrs = std::vec::Vec::new();
            for _ in 0..5 {
                let p = crate::heap_malloc(h, 32);
                assert!(!p.is_null());
                ptrs.push(p);
            }
            let mut n = 0usize;
            assert!(crate::heap_visit_blocks(
                h,
                true,
                Some(count_visit),
                &mut n as *mut usize as *mut core::ffi::c_void
            ));
            assert_eq!(n, 5);
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn heap_aligned_at_and_recalloc() {
        unsafe {
            let h = crate::heap_new();
            let p = crate::heap_malloc_aligned_at(h, 48, 32, 8);
            assert!(!p.is_null());
            assert_eq!((p as usize + 8) % 32, 0);
            crate::heap_destroy(h);
            let z = alloc::malloc_aligned(64, 32);
            assert!(!z.is_null());
            core::ptr::write_bytes(z, 0, 64);
            let q = alloc::rezalloc_aligned(z, 192, 32);
            assert!(!q.is_null());
            assert_eq!(q as usize % 32, 0);
            for i in 0..192 {
                assert_eq!(*q.add(i), 0);
            }
            alloc::free(q);
        }
    }

    #[test]
    fn manage_os_memory_as_arena() {
        unsafe {
            let size = 2 * 1024 * 1024;
            let raw = crate::os::mmap_anon(size);
            assert!(!raw.is_null());
            let mut id: *mut crate::Arena = core::ptr::null_mut();
            assert!(alloc::manage_os_memory_ex(
                raw, size, true, false, false, -1, true, &mut id
            ));
            assert!(!id.is_null());
            let h = crate::heap_new_in_arena(id);
            let p = crate::heap_malloc(h, 64);
            assert!(!p.is_null());
            assert!(crate::mi_arena::contains(id, p as *const u8));
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn expand_fits_or_fails_in_place() {
        unsafe {
            let p = alloc::malloc(32);
            assert!(!p.is_null());
            assert_eq!(alloc::expand(p, 8), p);
            assert_eq!(alloc::expand(p, 32), p);
            assert!(alloc::expand(p, 1 << 20).is_null());
            alloc::free(p);
            assert!(alloc::expand(core::ptr::null_mut(), 16).is_null());
        }
    }

    #[test]
    fn deferred_free_runs_on_collect() {
        use core::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        static HITS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn on_defer(_force: bool, _hb: u64, _arg: *mut core::ffi::c_void) {
            HITS.fetch_add(1, AtomicOrdering::Relaxed);
        }
        unsafe {
            crate::hooks::register_deferred_free(
                on_defer as *mut core::ffi::c_void,
                core::ptr::null_mut(),
            );
            HITS.store(0, AtomicOrdering::Relaxed);
            alloc::collect(true);
            assert!(HITS.load(AtomicOrdering::Relaxed) >= 1);
            crate::hooks::register_deferred_free(core::ptr::null_mut(), core::ptr::null_mut());
        }
    }

    #[test]
    fn error_handler_on_overflow() {
        use core::sync::atomic::{AtomicI32, Ordering as AtomicOrdering};
        static LAST: AtomicI32 = AtomicI32::new(0);
        unsafe extern "C" fn on_err(err: i32, _arg: *mut core::ffi::c_void) {
            LAST.store(err, AtomicOrdering::Relaxed);
        }
        unsafe {
            crate::hooks::register_error(on_err as *mut core::ffi::c_void, core::ptr::null_mut());
            LAST.store(0, AtomicOrdering::Relaxed);
            let p = alloc::calloc(usize::MAX, usize::MAX);
            assert!(p.is_null());
            assert_eq!(LAST.load(AtomicOrdering::Relaxed), crate::os::ENOMEM);
            crate::hooks::register_error(core::ptr::null_mut(), core::ptr::null_mut());
        }
    }

    #[test]
    fn debug_fill_uninit_and_freed() {
        if !cfg!(any(debug_assertions, feature = "debug-fill")) {
            return;
        }
        unsafe {
            let n = 64usize;
            let p = alloc::malloc(n);
            assert!(!p.is_null());
            for i in 0..n {
                assert_eq!(*p.add(i), crate::page::DEBUG_UNINIT);
            }
            alloc::free(p);
            let skip = core::mem::size_of::<*mut u8>();
            for i in skip..n {
                assert_eq!(*p.add(i), crate::page::DEBUG_FREED);
            }
        }
    }

    #[test]
    fn stats_track_mmap_and_malloc() {
        unsafe {
            let p = alloc::malloc(128);
            assert!(!p.is_null());
            let mut s: crate::Stats = core::mem::zeroed();
            crate::mi_stats::fill(&mut s);
            assert!(s.mmap_calls.total >= 1);
            assert!(s.reserved.current > 0);
            assert!(s.committed.current > 0);
            assert!(s.malloc_requested.current > 0);
            alloc::free(p);
        }
    }

    #[test]
    fn heap_stats_are_per_heap() {
        unsafe {
            let h = crate::heap_new();
            assert!(!h.is_null());
            for _ in 0..8 {
                let p = crate::heap_malloc(h, 64);
                assert!(!p.is_null());
            }
            let mut hs: crate::Stats = core::mem::zeroed();
            assert!(crate::heap_stats_get(h, &mut hs));
            assert!(hs.malloc_normal_count.total >= 8);
            assert!(hs.malloc_requested.current > 0);
            let t = crate::heap_theap(h);
            let mut ts: crate::Stats = core::mem::zeroed();
            assert!(crate::theap_stats_get(t, &mut ts));
            assert_eq!(ts.malloc_normal_count.total, hs.malloc_normal_count.total);
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn subproc_destroy_isolates_heaps() {
        unsafe {
            let a = crate::mi_subproc::new();
            crate::mi_subproc::add_current_thread(a);
            let h = crate::heap_new();
            assert!(!h.is_null());
            let p = crate::heap_malloc(h, 32);
            assert!(!p.is_null());
            crate::mi_subproc::destroy(a);
            let mut n = 0usize;
            unsafe extern "C" fn count(
                heap: *mut crate::Heap,
                arg: *mut core::ffi::c_void,
            ) -> bool {
                if !heap.is_null() {
                    *(arg as *mut usize) += 1;
                }
                true
            }
            crate::mi_subproc::visit_heaps(
                a,
                Some(count),
                &mut n as *mut usize as *mut core::ffi::c_void,
            );
            assert_eq!(n, 0);
            let p2 = alloc::malloc(16);
            assert!(!p2.is_null());
            alloc::free(p2);
        }
    }

    #[test]
    fn padding_usable_size_is_exact() {
        unsafe {
            let p = alloc::malloc(64);
            assert!(!p.is_null());
            assert_eq!(alloc::usable_size(p), 64);
            let z = alloc::malloc(0);
            assert!(!z.is_null());
            assert_eq!(alloc::usable_size(z), crate::PTR_SIZE);
            alloc::free(z);
            alloc::free(p);
        }
    }

    #[test]
    fn padding_reports_overflow_and_double_free() {
        use core::sync::atomic::{AtomicI32, Ordering as AtomicOrdering};
        static LAST: AtomicI32 = AtomicI32::new(0);
        unsafe extern "C" fn on_err(err: i32, _arg: *mut core::ffi::c_void) {
            LAST.store(err, AtomicOrdering::Relaxed);
        }
        unsafe {
            crate::hooks::register_error(on_err as *mut core::ffi::c_void, core::ptr::null_mut());
            LAST.store(0, AtomicOrdering::Relaxed);
            let p = alloc::malloc(16);
            assert!(!p.is_null());
            let page = crate::page_map::get(p);
            assert!(!page.is_null());
            let smash = (*page).block_size.saturating_sub(16);
            if smash != 0 {
                core::ptr::write_bytes(p.add(16), 0xFF, smash);
            }
            alloc::free(p);
            assert_eq!(LAST.load(AtomicOrdering::Relaxed), crate::os::EFAULT);

            LAST.store(0, AtomicOrdering::Relaxed);
            let q = alloc::malloc(32);
            alloc::free(q);
            alloc::free(q);
            assert_eq!(LAST.load(AtomicOrdering::Relaxed), crate::os::EAGAIN);
            crate::hooks::register_error(core::ptr::null_mut(), core::ptr::null_mut());
        }
    }

    #[test]
    fn guarded_sample_allocates_apart() {
        unsafe {
            crate::init();
            let th = crate::theap_get_default();
            crate::theap_guarded_set_size_bound(th, 0, usize::MAX);
            crate::theap_guarded_set_sample_rate(th, 1, 1);
            let a = alloc::malloc(32);
            let b = alloc::malloc(32);
            assert!(!a.is_null() && !b.is_null());
            assert!((a as usize).abs_diff(b as usize) > 4096);
            assert!(alloc::usable_size(a) >= 32);
            core::ptr::write_bytes(a, 0xAB, 32);
            core::ptr::write_bytes(b, 0xCD, 32);
            alloc::free(a);
            alloc::free(b);
            crate::theap_guarded_set_sample_rate(th, 0, 0);
        }
    }

    #[cfg(target_os = "linux")]
    fn prot_at(addr: usize) -> Option<std::string::String> {
        let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
        for line in maps.lines() {
            let mut it = line.split_whitespace();
            let range = it.next()?;
            let perms = it.next()?;
            let mut r = range.split('-');
            let start = usize::from_str_radix(r.next()?, 16).ok()?;
            let end = usize::from_str_radix(r.next()?, 16).ok()?;
            if addr >= start && addr < end {
                return Some(std::string::String::from(perms));
            }
        }
        None
    }

    #[test]
    fn invalid_free_is_ignored() {
        unsafe {
            let mut stack = 0u8;
            alloc::free(core::ptr::null_mut());
            alloc::free(0x10 as *mut u8);
            alloc::free(&mut stack);
            let p = alloc::malloc(64);
            assert!(!p.is_null());
            alloc::free(p.add(8));
            assert_eq!(alloc::usable_size(p.add(8)), 0);
            core::ptr::write_bytes(p, 0xAB, 64);
            alloc::free(p);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn metadata_guard_page_is_inaccessible() {
        unsafe {
            let p = alloc::malloc(32);
            assert!(!p.is_null());
            let page = crate::page_map::get(p);
            assert!(!page.is_null());
            let base = (*page).map_base as usize;
            let os = crate::os::page_size();
            let guard = prot_at(base).expect("map_base in maps");
            assert!(
                !guard.contains('r') && !guard.contains('w'),
                "leading meta guard should be PROT_NONE, got {guard}"
            );
            let meta = prot_at(base + os).expect("page header in maps");
            assert!(
                meta.contains('r') && meta.contains('w'),
                "page header should be RW, got {meta}"
            );
            alloc::free(p);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn end_of_page_guard_is_inaccessible() {
        unsafe {
            let p = alloc::malloc(32);
            assert!(!p.is_null());
            let page = crate::page_map::get(p);
            assert!(!page.is_null());
            let os = crate::os::page_size();
            let end = (*page).map_base as usize + (*page).map_size - os;
            let guard = prot_at(end).expect("end guard in maps");
            assert!(
                !guard.contains('r') && !guard.contains('w'),
                "end-of-page guard should be PROT_NONE, got {guard}"
            );
            let last = (*page).area as usize + ((*page).capacity as usize - 1) * (*page).block_size;
            assert!(
                last + (*page).block_size <= end,
                "blocks must not overlap the end guard"
            );
            alloc::free(p);
        }
    }

    #[test]
    fn free_list_is_not_strictly_sequential() {
        unsafe {
            let mut v: Vec<*mut u8> = Vec::new();
            for _ in 0..32 {
                let p = alloc::malloc(16);
                assert!(!p.is_null());
                v.push(p);
            }
            let page = crate::page_map::get(v[0]);
            let bs = (*page).block_size;
            let sequential = v
                .windows(2)
                .all(|w| (w[1] as usize).abs_diff(w[0] as usize) == bs);
            assert!(
                !sequential,
                "secure free-list init should not hand out 32 adjacent blocks in order"
            );
            for p in v {
                alloc::free(p);
            }
        }
    }

    #[test]
    fn free_size_too_large_still_frees() {
        unsafe {
            let p = alloc::malloc(32);
            assert!(!p.is_null());
            alloc::free_size(p, 1 << 20);
            assert_eq!(alloc::usable_size(p as *const u8), 0);
            let q = alloc::malloc(32);
            alloc::free_size(q, 32);
            assert_eq!(alloc::usable_size(q as *const u8), 0);
        }
    }

    #[test]
    fn numa_affinity_is_stored() {
        unsafe {
            let h = crate::heap_new();
            assert_eq!(crate::heap_numa_node(h), -1);
            crate::heap_set_numa_affinity(h, 3);
            assert_eq!(crate::heap_numa_node(h), 3);
            crate::heap_set_numa_affinity(h, -2);
            assert_eq!(crate::heap_numa_node(h), -1);
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn stats_merge_zeros_theap_into_subproc() {
        unsafe {
            let h = crate::heap_new();
            let p = crate::heap_malloc(h, 64);
            assert!(!p.is_null());
            let mut st = core::mem::zeroed();
            assert!(crate::theap_stats_get(crate::heap_theap(h), &mut st));
            assert!(st.malloc_requested.current > 0);
            crate::heap_stats_merge_to_subproc(h);
            assert!(crate::theap_stats_get(crate::heap_theap(h), &mut st));
            assert_eq!(st.malloc_requested.current, 0);
            crate::stats_merge();
            crate::heap_destroy(h);
        }
    }

    #[test]
    fn under_utilized_skips_current_and_matches_c() {
        unsafe {
            assert!(!crate::page_is_under_utilized(
                core::ptr::null_mut(),
                core::ptr::null(),
                50
            ));
            let mut v: Vec<*mut u8> = Vec::new();
            let mut older: *mut u8 = core::ptr::null_mut();
            let mut first_page: *mut crate::page::Page = core::ptr::null_mut();
            for _ in 0..4000 {
                let p = alloc::malloc(32);
                assert!(!p.is_null());
                let page = crate::page_map::get(p);
                if first_page.is_null() {
                    first_page = page;
                } else if page != first_page && older.is_null() {
                    older = v[0];
                }
                v.push(p);
                if !older.is_null() {
                    break;
                }
            }
            assert!(!older.is_null(), "need a second page for under-utilized");
            // Current-queue head (newest page) is skipped.
            let newest = *v.last().unwrap();
            assert!(!crate::page_is_under_utilized(
                core::ptr::null_mut(),
                newest,
                100
            ));
            alloc::free(older);
            assert!(crate::page_is_under_utilized(
                core::ptr::null_mut(),
                v[1],
                100
            ));
            let other = crate::heap_new();
            assert!(!crate::page_is_under_utilized(other, v[1], 100));
            crate::heap_destroy(other);
            for p in v {
                if p != older {
                    alloc::free(p);
                }
            }
        }
    }

    #[test]
    fn collect_all_and_threadpool_do_not_panic() {
        unsafe {
            let h = crate::heap_new();
            crate::theap_set_in_threadpool(crate::heap_theap(h));
            let p = crate::heap_malloc(h, 48);
            assert!(!p.is_null());
            crate::heap_collect(h, true);
            crate::heap_destroy(h);
            crate::collect_all(true);
            alloc::collect_reduce(0);
        }
    }
}
