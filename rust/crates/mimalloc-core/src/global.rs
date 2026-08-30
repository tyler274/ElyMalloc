//! `core::alloc::GlobalAlloc` surface for `#[global_allocator]` (including wasm32).

use crate::alloc as mi;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

/// Process-wide mimalloc. Safe to use as `#[global_allocator]` on
/// `wasm32-unknown-unknown` and `wasm32-wasip1` with no C toolchain.
pub struct Mimalloc;

unsafe impl GlobalAlloc for Mimalloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        mi::malloc_aligned(layout.size(), layout.align())
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = self.alloc(layout);
        if !p.is_null() && layout.size() != 0 {
            ptr::write_bytes(p, 0, layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        mi::free(ptr);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() <= 16 {
            return mi::realloc(ptr, new_size);
        }
        let q = mi::malloc_aligned(new_size, layout.align());
        if q.is_null() {
            return ptr::null_mut();
        }
        let n = core::cmp::min(layout.size(), new_size);
        if n != 0 && !ptr.is_null() {
            ptr::copy_nonoverlapping(ptr, q, n);
        }
        mi::free(ptr);
        q
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[test]
    fn alloc_respects_layout_align() {
        let a = Mimalloc;
        for align in [1usize, 8, 16, 32, 64, 256, 4096] {
            for size in [1usize, 48, 96, 4096] {
                let Ok(layout) = Layout::from_size_align(size, align) else {
                    continue;
                };
                unsafe {
                    let p = a.alloc(layout);
                    assert!(!p.is_null());
                    assert_eq!(p as usize % align, 0, "size={size} align={align}");
                    core::ptr::write_bytes(p, 0x3C, size);
                    a.dealloc(p, layout);
                }
            }
        }
    }

    #[test]
    fn alloc_zeroed_is_zero() {
        let a = Mimalloc;
        let layout = Layout::from_size_align(1024, 32).unwrap();
        unsafe {
            let p = a.alloc_zeroed(layout);
            assert!(!p.is_null());
            for i in 0..1024 {
                assert_eq!(*p.add(i), 0);
            }
            a.dealloc(p, layout);
        }
    }

    #[test]
    fn realloc_preserves_prefix_and_align() {
        let a = Mimalloc;
        let layout = Layout::from_size_align(48, 64).unwrap();
        unsafe {
            let p = a.alloc(layout);
            assert!(!p.is_null());
            core::ptr::write_bytes(p, 0x11, 48);
            let q = a.realloc(p, layout, 2048);
            assert!(!q.is_null());
            assert_eq!(q as usize % 64, 0);
            assert_eq!(*q, 0x11);
            a.dealloc(q, Layout::from_size_align(2048, 64).unwrap());
        }
    }

    #[test]
    fn realloc_shrinks() {
        let a = Mimalloc;
        let layout = Layout::from_size_align(4096, 8).unwrap();
        unsafe {
            let p = a.alloc(layout);
            core::ptr::write_bytes(p, 0x22, 64);
            let q = a.realloc(p, layout, 32);
            assert!(!q.is_null());
            assert_eq!(*q, 0x22);
            a.dealloc(q, Layout::from_size_align(32, 8).unwrap());
        }
    }

    #[test]
    fn many_global_allocs() {
        let a = Mimalloc;
        let mut v: Vec<(*mut u8, Layout)> = Vec::new();
        unsafe {
            for i in 0..256 {
                let layout = Layout::from_size_align((i % 64) + 1, 16).unwrap();
                let p = a.alloc(layout);
                assert!(!p.is_null());
                v.push((p, layout));
            }
            for (p, layout) in v {
                a.dealloc(p, layout);
            }
        }
    }
}
