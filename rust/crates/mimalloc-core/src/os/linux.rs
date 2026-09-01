//! Linux OS primitives via libc syscall wrappers that do not allocate.
//!
//! `mmap` is `MAP_PRIVATE|MAP_ANONYMOUS`. Aligned maps over-allocate by
//! `align + OS page`, then unmap the lead/trail so neighbouring mappings
//! have a virtual gap (C `_mi_os_get_aligned_hint`).

use super::{mix_rng, random_u64, PROT_NONE, PROT_READ, PROT_WRITE};
use crate::align_up;
use crate::spin::SpinLock;
use crate::{MEDIUM_PAGE_SIZE, SLICE_SIZE};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

pub const MAP_NORESERVE: i32 = libc::MAP_NORESERVE;

static mut OS_PAGE_SIZE: usize = 4096;

pub fn init() {
    unsafe {
        let n = libc::sysconf(libc::_SC_PAGESIZE);
        if n > 0 {
            OS_PAGE_SIZE = n as usize;
        }
        let mix = (gettid() as u64)
            .wrapping_mul(0xA076_1D64_78BD_642F)
            .wrapping_add(OS_PAGE_SIZE as u64);
        mix_rng(mix);
    }
}

#[inline]
pub fn page_size() -> usize {
    unsafe { OS_PAGE_SIZE }
}

#[inline]
pub fn abort() -> ! {
    unsafe {
        libc::_exit(1);
    }
}

#[inline]
pub fn gettid() -> u32 {
    // `SYS_gettid` is the Linux ABI on every arch (glibc and musl).
    unsafe { libc::syscall(libc::SYS_gettid) as u32 }
}

/// Anonymous mapping; null on failure (does not set errno itself).
pub unsafe fn mmap_anon(size: usize) -> *mut u8 {
    mmap_anon_prot(size, PROT_READ | PROT_WRITE, 0)
}

unsafe fn mmap_anon_prot(size: usize, prot: i32, extra_flags: i32) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }
    let p = libc::mmap(
        ptr::null_mut(),
        size,
        prot,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | extra_flags,
        -1,
        0,
    );
    if p == libc::MAP_FAILED {
        ptr::null_mut()
    } else {
        let committed = (prot & (PROT_READ | PROT_WRITE)) != 0;
        crate::stats::mmap_map(size, committed);
        p as *mut u8
    }
}

pub unsafe fn munmap(p: *mut u8, size: usize) {
    munmap_ex(p, size, true);
}

pub unsafe fn munmap_ex(p: *mut u8, size: usize, committed: bool) {
    if p.is_null() || size == 0 {
        return;
    }
    if recycle_slice(p, size) {
        return;
    }
    if in_segment(p) {
        // Interior of a 4 MiB page segment: never punch a hole (VMA explosion).
        return;
    }
    crate::stats::mmap_unmap(size, committed);
    libc::munmap(p as *mut libc::c_void, size);
}

/// Reserve `size` bytes aligned to `align` (both must be multiples of the OS page).
pub unsafe fn mmap_aligned(size: usize, align: usize) -> *mut u8 {
    mmap_aligned_prot(size, align, PROT_READ | PROT_WRITE, 0)
}

pub unsafe fn mmap_aligned_prot(size: usize, align: usize, prot: i32, extra_flags: i32) -> *mut u8 {
    if size == 0 || align == 0 || !align.is_power_of_two() {
        return ptr::null_mut();
    }
    let os = page_size();
    let size = align_up(size, os);
    let align = align.max(os);
    if extra_flags == 0 && prot == (PROT_READ | PROT_WRITE) {
        if let Some(p) = segment_try_alloc(size, align) {
            return p;
        }
    }
    mmap_aligned_fresh(size, align, prot, extra_flags)
}

unsafe fn mmap_aligned_fresh(size: usize, align: usize, prot: i32, extra_flags: i32) -> *mut u8 {
    let os = page_size();
    // Extra OS page as a virtual gap between mappings (C `_mi_os_get_aligned_hint`).
    let total = size.saturating_add(align).saturating_add(os);
    if total < size {
        return ptr::null_mut();
    }
    let committed = (prot & (PROT_READ | PROT_WRITE)) != 0;
    let raw = mmap_anon_prot(total, prot, extra_flags);
    if raw.is_null() {
        return ptr::null_mut();
    }
    let addr = raw as usize;
    let mut aligned = align_up(addr, align);
    let room = (addr + total).saturating_sub(aligned).saturating_sub(size);
    let extra_slots = room / align;
    if extra_slots > 0 {
        aligned += ((random_u64() as usize) % (extra_slots + 1)) * align;
    }
    let lead = aligned - addr;
    let trail = total - lead - size;
    if lead >= os {
        // Direct munmap: this mapping is not a page segment.
        crate::stats::mmap_unmap(lead, committed);
        libc::munmap(raw as *mut libc::c_void, lead);
    }
    if trail >= os {
        crate::stats::mmap_unmap(trail, committed);
        libc::munmap((aligned + size) as *mut libc::c_void, trail);
    }
    aligned as *mut u8
}

/// Reset segment metadata locks after `fork` in the child.
pub unsafe fn force_unlock() {
    SEG_LOCK.force_unlock();
}

pub unsafe fn madvise_dontneed(p: *mut u8, size: usize) {
    if p.is_null() || size == 0 {
        return;
    }
    crate::stats::purge(size);
    libc::madvise(p as *mut libc::c_void, size, libc::MADV_DONTNEED);
}

pub fn set_errno(err: i32) {
    unsafe {
        *errno_location() = err;
    }
}

#[inline]
fn errno_location() -> *mut i32 {
    unsafe { libc::__errno_location() }
}

/// `mprotect(PROT_NONE)` - used for lead/mid/end guards and sampled guarded allocs.
pub unsafe fn protect(p: *mut u8, size: usize) -> bool {
    if p.is_null() || size == 0 {
        return true;
    }
    libc::mprotect(p as *mut libc::c_void, size, PROT_NONE) == 0
}

/// Restore `PROT_READ|PROT_WRITE` before `munmap` of a guarded region.
pub unsafe fn unprotect(p: *mut u8, size: usize) -> bool {
    if p.is_null() || size == 0 {
        return true;
    }
    libc::mprotect(p as *mut libc::c_void, size, PROT_READ | PROT_WRITE) == 0
}

pub unsafe fn commit(p: *mut u8, size: usize) -> bool {
    unprotect(p, size)
}

/// 4 MiB OS segments (C mimalloc pages live in large segments, not one mmap
/// each). Guard `mprotect` still splits VMAs inside the segment; we avoid
/// mmap+unmap of every 64 KiB page, which exhausted `vm.max_map_count` under
/// .NET / KWin.
const SEGMENT_SIZE: usize = crate::LARGE_PAGE_SIZE;

#[repr(C)]
struct Seg {
    base: *mut u8,
    size: usize,
    bump: AtomicUsize,
    next: *mut Seg,
}

static SEG_LOCK: SpinLock = SpinLock::new();
static SEGMENTS: AtomicPtr<Seg> = AtomicPtr::new(ptr::null_mut());
static CURRENT: AtomicPtr<Seg> = AtomicPtr::new(ptr::null_mut());
static FREE64: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static FREE512: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static mut META_BUMP: *mut u8 = ptr::null_mut();
static mut META_END: *mut u8 = ptr::null_mut();

unsafe fn alloc_seg_meta() -> *mut Seg {
    let need = core::mem::size_of::<Seg>();
    if META_BUMP.is_null() || META_BUMP.add(need) > META_END {
        let chunk = mmap_anon_prot(4096, PROT_READ | PROT_WRITE, 0);
        if chunk.is_null() {
            return ptr::null_mut();
        }
        META_BUMP = chunk;
        META_END = chunk.add(4096);
    }
    let s = META_BUMP as *mut Seg;
    META_BUMP = META_BUMP.add(need);
    ptr::write_bytes(s as *mut u8, 0, need);
    s
}

unsafe fn push_free(head: &AtomicPtr<u8>, p: *mut u8) {
    loop {
        let old = head.load(Ordering::Acquire);
        ptr::write(p as *mut *mut u8, old);
        if head
            .compare_exchange_weak(old, p, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
    }
}

unsafe fn pop_free(head: &AtomicPtr<u8>) -> *mut u8 {
    loop {
        let p = head.load(Ordering::Acquire);
        if p.is_null() {
            return ptr::null_mut();
        }
        let next = ptr::read(p as *mut *mut u8);
        if head
            .compare_exchange_weak(p, next, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            return p;
        }
    }
}

fn free_head_for(size: usize) -> Option<&'static AtomicPtr<u8>> {
    if size == SLICE_SIZE {
        Some(&FREE64)
    } else if size == MEDIUM_PAGE_SIZE {
        Some(&FREE512)
    } else {
        None
    }
}

unsafe fn recycle_slice(p: *mut u8, size: usize) -> bool {
    if !in_segment(p) {
        return false;
    }
    let Some(head) = free_head_for(size) else {
        return true;
    };
    push_free(head, p);
    true
}

fn in_segment(p: *mut u8) -> bool {
    if p.is_null() {
        return false;
    }
    let addr = p as usize;
    let mut s = SEGMENTS.load(Ordering::Acquire);
    while !s.is_null() {
        unsafe {
            let base = (*s).base as usize;
            let end = base.wrapping_add((*s).size);
            if addr >= base && addr < end {
                return true;
            }
            s = (*s).next;
        }
    }
    false
}

unsafe fn new_segment() -> *mut Seg {
    let s = alloc_seg_meta();
    if s.is_null() {
        return ptr::null_mut();
    }
    let raw = mmap_aligned_fresh(SEGMENT_SIZE, SLICE_SIZE, PROT_READ | PROT_WRITE, 0);
    if raw.is_null() {
        return ptr::null_mut();
    }
    (*s).base = raw;
    (*s).size = SEGMENT_SIZE;
    (*s).bump = AtomicUsize::new(0);
    loop {
        let old = SEGMENTS.load(Ordering::Acquire);
        (*s).next = old;
        if SEGMENTS
            .compare_exchange_weak(old, s, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
    CURRENT.store(s, Ordering::Release);
    s
}

unsafe fn segment_try_alloc(size: usize, align: usize) -> Option<*mut u8> {
    if size > MEDIUM_PAGE_SIZE || align > SLICE_SIZE || size % SLICE_SIZE != 0 {
        return None;
    }
    if let Some(head) = free_head_for(size) {
        let p = pop_free(head);
        if !p.is_null() {
            ptr::write_bytes(p, 0, core::mem::size_of::<*mut u8>());
            return Some(p);
        }
    }
    let _g = SEG_LOCK.lock();
    if let Some(head) = free_head_for(size) {
        let p = pop_free(head);
        if !p.is_null() {
            ptr::write_bytes(p, 0, core::mem::size_of::<*mut u8>());
            return Some(p);
        }
    }
    let mut s = CURRENT.load(Ordering::Acquire);
    if s.is_null() {
        s = new_segment();
        if s.is_null() {
            return None;
        }
    }
    loop {
        let pos = (*s).bump.load(Ordering::Relaxed);
        let aligned = align_up(pos, align.max(SLICE_SIZE));
        let Some(new_pos) = aligned.checked_add(size) else {
            return None;
        };
        if new_pos > (*s).size {
            s = new_segment();
            if s.is_null() {
                return None;
            }
            continue;
        }
        if (*s)
            .bump
            .compare_exchange_weak(pos, new_pos, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return Some((*s).base.add(aligned));
        }
    }
}
