//! Two-level page map: each 64 KiB slice of the address space points at a `Page`.
//!
//! C can use a *flat* map (`_mi_page_map[addr >> 16]`) or a 2-level tree; this
//! rewrite always uses two levels so a 48-bit VA does not need a 4 GiB table.
//!
//! - L1: `addr >> (16+13)` → pointer to an L2 table (lazily `mmap`'d).
//! - L2: next 13 bits → `*mut Page` for that slice.
//!
//! A multi-slice page (medium/large/huge) stores the *same* `Page*` in every
//! covered slot (C: offset byte `1 < ofs <= 127` in the flat map). `free`
//! looks up the user pointer's slice; null means not ours.
//!
//! Empty L1 so `free(NULL)` and pre-init frees are safe (C issue #1341).

use crate::os;
use crate::page::Page;
use crate::SLICE_SHIFT;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

const L2_BITS: usize = 13;
const L2_SIZE: usize = 1 << L2_BITS;
const L1_BITS: usize = 18;
const L1_SIZE: usize = 1 << L1_BITS;

static L1: AtomicPtr<AtomicPtr<u8>> = AtomicPtr::new(ptr::null_mut());

#[inline]
fn split(addr: usize) -> (usize, usize) {
    let slice = addr >> SLICE_SHIFT;
    let l2 = slice & (L2_SIZE - 1);
    let l1 = slice >> L2_BITS;
    (l1, l2)
}

fn l1_table() -> *mut AtomicPtr<u8> {
    L1.load(Ordering::Acquire)
}

/// Reserve the L1 table. Idempotent; later inits lose the race and unmap their copy.
pub fn init() {
    if !l1_table().is_null() {
        return;
    }
    let bytes = L1_SIZE * core::mem::size_of::<*mut u8>();
    let Some(map) = os::Mapping::anon(bytes) else {
        os::abort();
    };
    let raw = map.as_ptr();
    match L1.compare_exchange(
        ptr::null_mut(),
        raw as *mut AtomicPtr<u8>,
        Ordering::Release,
        Ordering::Acquire,
    ) {
        Ok(_) => {
            let _ = map.leak();
        }
        Err(_) => {}
    }
}

unsafe fn l2_slot(l1_idx: usize) -> *mut AtomicPtr<u8> {
    let table = l1_table();
    if table.is_null() || l1_idx >= L1_SIZE {
        return ptr::null_mut();
    }
    table.add(l1_idx)
}

unsafe fn get_or_create_l2(l1_idx: usize) -> *mut AtomicPtr<Page> {
    let slot = l2_slot(l1_idx);
    if slot.is_null() {
        return ptr::null_mut();
    }
    let existing = (*slot).load(Ordering::Acquire) as *mut AtomicPtr<Page>;
    if !existing.is_null() {
        return existing;
    }
    let bytes = L2_SIZE * core::mem::size_of::<*mut Page>();
    let Some(map) = os::Mapping::anon(bytes) else {
        return ptr::null_mut();
    };
    let raw = map.as_ptr();
    match (*slot).compare_exchange(ptr::null_mut(), raw, Ordering::Release, Ordering::Acquire) {
        Ok(_) => {
            let _ = map.leak();
            raw as *mut AtomicPtr<Page>
        }
        Err(cur) => cur as *mut AtomicPtr<Page>,
    }
}

/// Register every slice in `[base, base+bytes)` as belonging to `page`.
pub unsafe fn set_range(base: *mut u8, bytes: usize, page: *mut Page) {
    if base.is_null() || bytes == 0 || page.is_null() {
        return;
    }
    let start = base as usize;
    let end = start.saturating_add(bytes);
    let mut addr = start & !((1usize << SLICE_SHIFT) - 1);
    while addr < end {
        let (l1, l2) = split(addr);
        let table = get_or_create_l2(l1);
        if !table.is_null() {
            (*table.add(l2)).store(page, Ordering::Release);
        }
        addr += 1 << SLICE_SHIFT;
    }
}

/// Clear those slices (page is being unmapped).
pub unsafe fn clear_range(base: *mut u8, bytes: usize) {
    if base.is_null() || bytes == 0 {
        return;
    }
    let start = base as usize;
    let end = start.saturating_add(bytes);
    let mut addr = start & !((1usize << SLICE_SHIFT) - 1);
    while addr < end {
        let (l1, l2) = split(addr);
        let slot = l2_slot(l1);
        if !slot.is_null() {
            let table = (*slot).load(Ordering::Acquire) as *mut AtomicPtr<Page>;
            if !table.is_null() {
                (*table.add(l2)).store(ptr::null_mut(), Ordering::Release);
            }
        }
        addr += 1 << SLICE_SHIFT;
    }
}

/// Owning page of `ptr`, or null if the slice is unused / map not committed.
#[inline]
pub unsafe fn get(ptr: *const u8) -> *mut Page {
    if ptr.is_null() {
        return ptr::null_mut();
    }
    let (l1, l2) = split(ptr as usize);
    let slot = l2_slot(l1);
    if slot.is_null() {
        return ptr::null_mut();
    }
    let table = (*slot).load(Ordering::Acquire) as *mut AtomicPtr<Page>;
    if table.is_null() {
        return ptr::null_mut();
    }
    (*table.add(l2)).load(Ordering::Acquire)
}
