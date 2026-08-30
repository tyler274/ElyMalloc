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
