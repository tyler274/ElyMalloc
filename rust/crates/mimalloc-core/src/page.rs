//! Mimalloc-style pages: one size class, local + concurrent free lists.

use crate::arena::{self, Arena};
use crate::os;
use crate::page_map;
use crate::{align_up, SLICE_SIZE};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

#[repr(C)]
pub struct Block {
    pub next: *mut Block,
}

#[repr(C, align(64))]
pub struct Page {
    pub magic: u32,
    pub block_size: usize,
    pub capacity: u32,
    pub used: u32,
    pub local_free: *mut Block,
    pub thread_free: AtomicPtr<Block>,
    pub next: *mut Page,
    pub prev: *mut Page,
    pub heap: AtomicPtr<crate::heap::ThreadHeap>,
    pub area: *mut u8,
    pub map_base: *mut u8,
    pub map_size: usize,
    pub flags: AtomicU32,
}

pub const PAGE_MAGIC: u32 = 0x4D495041; // 'MIPA'
const FLAG_ABANDONED: u32 = 1;
const FLAG_ARENA: u32 = 2;

impl Page {
    #[inline]
    pub fn set_abandoned(&self, yes: bool) {
        if yes {
            self.flags.fetch_or(FLAG_ABANDONED, Ordering::Release);
        } else {
            self.flags.fetch_and(!FLAG_ABANDONED, Ordering::Release);
        }
    }

    #[inline]
    pub fn is_arena(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & FLAG_ARENA != 0
    }

    #[inline]
    fn set_arena(&self) {
        self.flags.fetch_or(FLAG_ARENA, Ordering::Release);
    }
}

unsafe fn map_page_memory(size: usize, align: usize, arena: *mut Arena) -> (*mut u8, bool) {
    if !arena.is_null() {
        let p = arena::alloc(arena, size, align);
        return (p, true);
    }
    (os::mmap_aligned(size, align), false)
}

unsafe fn unmap_page_memory(base: *mut u8, size: usize, from_arena: bool) {
    if from_arena {
        return;
    }
    os::munmap(base, size);
}

/// Align the first block so every block is aligned to the largest power of two
/// that divides `block_size` (needed for `malloc_aligned` over-size classes).
fn block_align(block_size: usize) -> usize {
    if block_size == 0 {
        return 16;
    }
    let po2 = block_size & block_size.wrapping_neg();
    po2.max(16)
}

/// Allocate a page of `map_size` bytes holding equal `block_size` blocks.
pub unsafe fn create(block_size: usize, map_size: usize, arena: *mut Arena) -> *mut Page {
    let map_size = align_up(map_size.max(SLICE_SIZE), SLICE_SIZE);
    let (base, from_arena) = map_page_memory(map_size, SLICE_SIZE, arena);
    if base.is_null() {
        return ptr::null_mut();
    }
    let page = base as *mut Page;
    ptr::write_bytes(page as *mut u8, 0, core::mem::size_of::<Page>());
    let align = block_align(block_size);
    let header = align_up(core::mem::size_of::<Page>(), align);
    let area = base.add(header);
    let usable = map_size.saturating_sub(header);
    if usable < block_size {
        unmap_page_memory(base, map_size, from_arena);
        return ptr::null_mut();
    }
    let capacity = usable / block_size;
    if capacity == 0 {
        unmap_page_memory(base, map_size, from_arena);
        return ptr::null_mut();
    }

    (*page).magic = PAGE_MAGIC;
    (*page).block_size = block_size;
    (*page).capacity = capacity as u32;
    (*page).used = 0;
    (*page).local_free = ptr::null_mut();
    (*page).thread_free = AtomicPtr::new(ptr::null_mut());
    (*page).next = ptr::null_mut();
    (*page).prev = ptr::null_mut();
    (*page).heap = AtomicPtr::new(ptr::null_mut());
    (*page).area = area;
    (*page).map_base = base;
    (*page).map_size = map_size;
    (*page).flags = AtomicU32::new(0);
    if from_arena {
        (*page).set_arena();
    }

    let mut i = capacity;
    while i > 0 {
        i -= 1;
        let b = area.add(i * block_size) as *mut Block;
        (*b).next = (*page).local_free;
        (*page).local_free = b;
    }

    page_map::set_range(base, map_size, page);
    crate::stats::page_add();
    page
}

/// Dedicated huge/aligned allocation: one block covering `size` bytes (plus header).
pub unsafe fn create_huge(
    size: usize,
    align: usize,
    offset: usize,
    arena: *mut Arena,
) -> *mut Page {
    let align = if align == 0 { 16 } else { align };
    if !align.is_power_of_two() {
        return ptr::null_mut();
    }
    let payload = size.max(16);
    let header = align_up(core::mem::size_of::<Page>(), 16);
    let total = align_up(
        header.saturating_add(align).saturating_add(payload),
        SLICE_SIZE.max(align.min(SLICE_SIZE * 16)),
    );
    let map_align = align.max(SLICE_SIZE);
    let (base, from_arena) = map_page_memory(total, map_align, arena);
    if base.is_null() {
        return ptr::null_mut();
    }
    let page = base as *mut Page;
    ptr::write_bytes(page as *mut u8, 0, core::mem::size_of::<Page>());
    let area0 = (base as usize) + header;
    let off = offset % align;
    let want = if off == 0 { 0 } else { align - off };
    let cur = area0 % align;
    let add = (want + align - cur) % align;
    let area = (area0 + add) as *mut u8;
    if (area as usize) + payload > (base as usize) + total {
        unmap_page_memory(base, total, from_arena);
        return ptr::null_mut();
    }
    (*page).magic = PAGE_MAGIC;
    (*page).block_size = size.max(16);
    (*page).capacity = 1;
    (*page).used = 0;
    (*page).local_free = area as *mut Block;
    (*page).thread_free = AtomicPtr::new(ptr::null_mut());
    (*page).next = ptr::null_mut();
    (*page).prev = ptr::null_mut();
    (*page).heap = AtomicPtr::new(ptr::null_mut());
    (*page).area = area;
    (*page).map_base = base;
    (*page).map_size = total;
    (*page).flags = AtomicU32::new(0);
    if from_arena {
        (*page).set_arena();
    }
    page_map::set_range(base, total, page);
    crate::stats::page_add();
    page
}

pub unsafe fn destroy(page: *mut Page) {
    if page.is_null() {
        return;
    }
    let from_arena = (*page).is_arena();
    let base = (*page).map_base;
    let size = (*page).map_size;
    page_map::clear_range(base, size);
    crate::stats::page_sub();
    unmap_page_memory(base, size, from_arena);
}

#[inline]
pub unsafe fn collect(page: *mut Page) {
    if page.is_null() {
        return;
    }
    let mut p = (*page).thread_free.swap(ptr::null_mut(), Ordering::AcqRel);
    let mut n = 0u32;
    while !p.is_null() {
        n += 1;
        let next = (*p).next;
        (*p).next = (*page).local_free;
        (*page).local_free = p;
        p = next;
    }
    (*page).used = (*page).used.saturating_sub(n);
}

#[inline]
pub unsafe fn pop_local(page: *mut Page) -> *mut u8 {
    if (*page).capacity == 1 {
        if (*page).used != 0 || (*page).area.is_null() {
            return ptr::null_mut();
        }
        if (*page).local_free.is_null() {
            return ptr::null_mut();
        }
        (*page).local_free = ptr::null_mut();
        (*page).used = 1;
        return (*page).area;
    }
    let b = (*page).local_free;
    if b.is_null() {
        return ptr::null_mut();
    }
    (*page).local_free = (*b).next;
    (*page).used = (*page).used.saturating_add(1);
    b as *mut u8
}

#[inline]
pub unsafe fn push_local(page: *mut Page, ptr: *mut u8) {
    if (*page).capacity == 1 {
        (*page).local_free = ptr as *mut Block;
        (*page).used = 0;
        return;
    }
    let b = ptr as *mut Block;
    (*b).next = (*page).local_free;
    (*page).local_free = b;
    (*page).used = (*page).used.saturating_sub(1);
}

#[inline]
pub unsafe fn push_thread_free(page: *mut Page, ptr: *mut u8) {
    let b = ptr as *mut Block;
    loop {
        let old = (*page).thread_free.load(Ordering::Relaxed);
        (*b).next = old;
        if (*page)
            .thread_free
            .compare_exchange_weak(old, b, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

#[inline]
pub unsafe fn contains(page: *mut Page, ptr: *const u8) -> bool {
    if page.is_null() || ptr.is_null() {
        return false;
    }
    let start = (*page).area as usize;
    let end = start + ((*page).capacity as usize) * (*page).block_size;
    let addr = ptr as usize;
    addr >= start && addr < end
}

#[inline]
pub unsafe fn is_block_start(page: *mut Page, ptr: *const u8) -> bool {
    if !contains(page, ptr) {
        return false;
    }
    let bs = (*page).block_size;
    if bs == 0 {
        return false;
    }
    let off = (ptr as usize) - ((*page).area as usize);
    off % bs == 0
}
