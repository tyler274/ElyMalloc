//! WebAssembly OS backend via `memory.grow`.
//!
//! No C toolchain, libc, or emscripten. Linear memory cannot shrink, so
//! `munmap` only updates stats. Alignment padding is stranded (same as
//! rusty_alloc / C mimalloc's WASI `sbrk` path).

use super::{mix_rng, PROT_READ, PROT_WRITE};
use crate::align_up;
use crate::spin::SpinLock;
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering};

/// Wasm linear-memory page (not the 4 KiB Linux page).
pub const WASM_PAGE: usize = 65536;
pub const MAP_NORESERVE: i32 = 0;

static GROW_LOCK: SpinLock = SpinLock::new();
static ERRNO: AtomicI32 = AtomicI32::new(0);

pub fn init() {
    mix_rng(
        (gettid() as u64)
            .wrapping_mul(0xA076_1D64_78BD_642F)
            .wrapping_add(WASM_PAGE as u64),
    );
}

#[inline]
pub fn page_size() -> usize {
    WASM_PAGE
}

#[inline]
pub fn abort() -> ! {
    core::arch::wasm32::unreachable()
}

#[inline]
pub fn gettid() -> u32 {
    1
}

fn memory_end() -> usize {
    core::arch::wasm32::memory_size::<0>() * WASM_PAGE
}

unsafe fn grow_from_end(bytes: usize) -> *mut u8 {
    if bytes == 0 {
        return ptr::null_mut();
    }
    let pages = (bytes + WASM_PAGE - 1) / WASM_PAGE;
    let prev = core::arch::wasm32::memory_grow::<0>(pages);
    if prev == usize::MAX {
        return ptr::null_mut();
    }
    (prev * WASM_PAGE) as *mut u8
}

pub unsafe fn mmap_anon(size: usize) -> *mut u8 {
    mmap_anon_prot(size, PROT_READ | PROT_WRITE, 0)
}

unsafe fn mmap_anon_prot(size: usize, prot: i32, _extra_flags: i32) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }
    let _g = GROW_LOCK.lock();
    let p = grow_from_end(align_up(size, WASM_PAGE));
    if p.is_null() {
        return ptr::null_mut();
    }
    let committed = (prot & (PROT_READ | PROT_WRITE)) != 0;
    crate::stats::mmap_map(size, committed);
    p
}

pub unsafe fn munmap(p: *mut u8, size: usize) {
    munmap_ex(p, size, true);
}

pub unsafe fn munmap_ex(p: *mut u8, size: usize, committed: bool) {
    // Linear memory cannot shrink; keep the range in the heap's free lists.
    if !p.is_null() && size != 0 {
        crate::stats::mmap_unmap(size, committed);
    }
}

pub unsafe fn mmap_aligned(size: usize, align: usize) -> *mut u8 {
    mmap_aligned_prot(size, align, PROT_READ | PROT_WRITE, 0)
}

pub unsafe fn mmap_aligned_prot(
    size: usize,
    align: usize,
    prot: i32,
    _extra_flags: i32,
) -> *mut u8 {
    if size == 0 || align == 0 || !align.is_power_of_two() {
        return ptr::null_mut();
    }
    let align = align.max(WASM_PAGE);
    let size = align_up(size, WASM_PAGE);
    let _g = GROW_LOCK.lock();
    let cur = memory_end();
    let pad = align_up(cur, align) - cur;
    let total = pad.saturating_add(size);
    if total < size {
        return ptr::null_mut();
    }
    let p = grow_from_end(total);
    if p.is_null() {
        return ptr::null_mut();
    }
    debug_assert_eq!(p as usize, cur);
    let committed = (prot & (PROT_READ | PROT_WRITE)) != 0;
    crate::stats::mmap_map(size, committed);
    (p as usize + pad) as *mut u8
}

pub unsafe fn madvise_dontneed(p: *mut u8, size: usize) {
    if p.is_null() || size == 0 {
        return;
    }
    crate::stats::purge(size);
}

pub fn set_errno(err: i32) {
    ERRNO.store(err, Ordering::Relaxed);
}

pub unsafe fn protect(_p: *mut u8, _size: usize) -> bool {
    true
}

pub unsafe fn unprotect(_p: *mut u8, _size: usize) -> bool {
    true
}

pub unsafe fn commit(_p: *mut u8, _size: usize) -> bool {
    true
}

#[allow(dead_code)]
pub unsafe fn force_unlock() {}
