//! Pure-Rust mimalloc-inspired allocator core (`no_std`, no `alloc` crate).

#![no_std]
#![allow(clippy::missing_safety_doc)]

pub mod alloc;
pub mod arena;
mod bin;
mod heap;
mod os;
pub mod options;
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

pub use alloc::{
    aligned_alloc, calloc, collect, free, good_size, malloc, malloc_aligned, malloc_aligned_at,
    manage_os_memory_ex, memalign, posix_memalign, pvalloc, realloc, reallocarr, reallocarray,
    reallocf, realpath, reserve_os_memory, reserve_os_memory_ex, rezalloc, rezalloc_aligned, strdup,
    strndup, ufree, umalloc, urealloc, usable_size, valloc, VERSION,
};
pub use arena::{self as mi_arena, Arena};
pub use heap::{
    any_heap_contains, heap_collect, heap_contains, heap_delete, heap_destroy, heap_main,
    heap_malloc, heap_malloc_aligned, heap_new, heap_new_in_arena, heap_of, heap_theap,
    heap_visit_abandoned_blocks, heap_visit_blocks, theap_collect, theap_get_default, theap_malloc,
    theap_malloc_aligned, theap_set_default, theap_visit_blocks, BlockVisitFun, Heap, HeapArea,
    Theap,
};
pub use subproc::{self as mi_subproc, Subproc, SubprocId};
pub use options as mi_options;
pub use stats::{self as mi_stats, Stats};

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
            assert_eq!(alloc::posix_memalign(&mut p, 3, 32), libc::EINVAL);
            assert_eq!(p, sentinel);
            p = sentinel;
            assert_eq!(
                alloc::posix_memalign(&mut p, 3 * crate::PTR_SIZE, 32),
                libc::EINVAL
            );
            assert_eq!(p, sentinel);
            p = sentinel;
            assert_eq!(
                alloc::posix_memalign(&mut p, crate::PTR_SIZE, usize::MAX),
                libc::ENOMEM
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
            let s = b"hello mimalloc\0".as_ptr() as *const libc::c_char;
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
    fn manage_os_memory_as_arena() {
        unsafe {
            let size = 2 * 1024 * 1024;
            let raw = crate::os::mmap_anon(size);
            assert!(!raw.is_null());
            let mut id: *mut crate::Arena = core::ptr::null_mut();
            assert!(alloc::manage_os_memory_ex(
                raw,
                size,
                true,
                false,
                false,
                -1,
                true,
                &mut id
            ));
            assert!(!id.is_null());
            let h = crate::heap_new_in_arena(id);
            let p = crate::heap_malloc(h, 64);
            assert!(!p.is_null());
            assert!(crate::mi_arena::contains(id, p as *const u8));
            crate::heap_destroy(h);
        }
    }
}
