//! Windows `VirtualAlloc` backend. No partial free; 64 KiB granularity.

use super::{mix_rng, PROT_READ, PROT_WRITE};
use crate::align_up;
use crate::spin::SpinLock;
use crate::{MEDIUM_PAGE_SIZE, SLICE_SIZE};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualFree, VirtualProtect, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS,
    PAGE_READWRITE,
};
use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows_sys::Win32::System::Threading::{ExitProcess, GetCurrentThreadId, SwitchToThread};

pub const MAP_NORESERVE: i32 = 0;

static mut OS_PAGE_SIZE: usize = 4096;
static mut GRANULE: usize = 65536;

pub fn init() {
    unsafe {
        let mut info: SYSTEM_INFO = core::mem::zeroed();
        GetSystemInfo(&mut info);
        if info.dwPageSize > 0 {
            OS_PAGE_SIZE = info.dwPageSize as usize;
        }
        if info.dwAllocationGranularity > 0 {
            GRANULE = info.dwAllocationGranularity as usize;
        }
        mix_rng(
            (thread_id() as u64)
                .wrapping_mul(0xA076_1D64_78BD_642F)
                .wrapping_add(OS_PAGE_SIZE as u64),
        );
        let _ = va_bits();
    }
}

#[inline]
pub fn page_size() -> usize {
    unsafe { OS_PAGE_SIZE }
}

#[inline]
fn granule() -> usize {
    unsafe { GRANULE }
}

#[inline]
pub fn abort() -> ! {
    unsafe {
        ExitProcess(1);
    }
}

pub fn yield_now() {
    unsafe {
        SwitchToThread();
    }
}

#[inline]
pub fn thread_id() -> u32 {
    gettid()
}

#[inline]
pub fn gettid() -> u32 {
    unsafe { GetCurrentThreadId() }
}

pub fn va_bits() -> usize {
    47
}

pub unsafe fn reuse(_p: *mut u8, _size: usize) {}

unsafe fn virt_alloc(size: usize, commit: bool) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }
    let ty = if commit {
        MEM_RESERVE | MEM_COMMIT
    } else {
        MEM_RESERVE
    };
    let p = VirtualAlloc(ptr::null(), size, ty, PAGE_READWRITE);
    if p.is_null() {
        let _ = GetLastError();
        ptr::null_mut()
    } else {
        crate::stats::mmap_map(size, commit);
        p as *mut u8
    }
}

pub unsafe fn mmap_anon(size: usize) -> *mut u8 {
    mmap_anon_prot(size, PROT_READ | PROT_WRITE, 0)
}

unsafe fn mmap_anon_prot(size: usize, prot: i32, extra_flags: i32) -> *mut u8 {
    let commit = extra_flags == 0 && (prot & (PROT_READ | PROT_WRITE)) != 0;
    virt_alloc(size, commit)
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
        return;
    }
    if let Some((base, total)) = take_overmap(p) {
        crate::stats::mmap_unmap(total, committed);
        let _ = VirtualFree(base as *mut core::ffi::c_void, 0, MEM_RELEASE);
        return;
    }
    crate::stats::mmap_unmap(size, committed);
    // MEM_RELEASE requires dwSize 0 and the base of the allocation.
    let _ = VirtualFree(p as *mut core::ffi::c_void, 0, MEM_RELEASE);
}

pub unsafe fn mmap_aligned(size: usize, align: usize) -> *mut u8 {
    mmap_aligned_prot(size, align, PROT_READ | PROT_WRITE, 0)
}

pub unsafe fn mmap_aligned_prot(size: usize, align: usize, prot: i32, extra_flags: i32) -> *mut u8 {
    if size == 0 || align == 0 || !align.is_power_of_two() {
        return ptr::null_mut();
    }
    let os = page_size();
    let size = align_up(size, os.max(granule()));
    let align = align.max(os);
    if extra_flags == 0 && prot == (PROT_READ | PROT_WRITE) {
        if let Some(p) = segment_try_alloc(size, align) {
            return p;
        }
    }
    // No partial free: if align <= allocation granularity, VirtualAlloc is enough.
    if align <= granule() {
        return virt_alloc(size, extra_flags == 0);
    }
    let total = size.saturating_add(align);
    let raw = virt_alloc(total, extra_flags == 0);
    if raw.is_null() {
        return ptr::null_mut();
    }
    let aligned = align_up(raw as usize, align) as *mut u8;
    register_overmap(raw, aligned, total);
    aligned
}

struct Overmap {
    base: *mut u8,
    user: *mut u8,
    total: usize,
    next: *mut Overmap,
}

static OVERMAPS: AtomicPtr<Overmap> = AtomicPtr::new(ptr::null_mut());

unsafe fn register_overmap(base: *mut u8, user: *mut u8, total: usize) {
    let n = core::mem::size_of::<Overmap>();
    let slot = virt_alloc(align_up(n, page_size()), true);
    if slot.is_null() {
        return;
    }
    let o = slot as *mut Overmap;
    (*o).base = base;
    (*o).user = user;
    (*o).total = total;
    loop {
        let old = OVERMAPS.load(Ordering::Acquire);
        (*o).next = old;
        if OVERMAPS
            .compare_exchange_weak(old, o, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
    }
}

unsafe fn take_overmap(p: *mut u8) -> Option<(*mut u8, usize)> {
    let mut prev: *mut Overmap = ptr::null_mut();
    let mut cur = OVERMAPS.load(Ordering::Acquire);
    while !cur.is_null() {
        if (*cur).user == p || (*cur).base == p {
            let next = (*cur).next;
            if prev.is_null() {
                if OVERMAPS
                    .compare_exchange(cur, next, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    prev = ptr::null_mut();
                    cur = OVERMAPS.load(Ordering::Acquire);
                    continue;
                }
            } else {
                (*prev).next = next;
            }
            let base = (*cur).base;
            let total = (*cur).total;
            let _ = VirtualFree(cur as *mut core::ffi::c_void, 0, MEM_RELEASE);
            return Some((base, total));
        }
        prev = cur;
        cur = (*cur).next;
    }
    None
}

#[allow(dead_code)]
pub unsafe fn force_unlock() {
    SEG_LOCK.force_unlock();
}

pub fn set_errno(err: i32) {
    // CRT errno; rustc links ucrt on this target.
    unsafe extern "C" {
        fn _errno() -> *mut i32;
    }
    unsafe {
        *_errno() = err;
    }
}

pub unsafe fn madvise_dontneed(p: *mut u8, size: usize) {
    if p.is_null() || size == 0 {
        return;
    }
    crate::stats::purge(size);
    // Decommit would require recommit; leave pages committed (C `MEM_RESET` optional).
}

pub unsafe fn protect(p: *mut u8, size: usize) -> bool {
    if p.is_null() || size == 0 {
        return true;
    }
    let mut old = 0u32;
    VirtualProtect(p as *mut core::ffi::c_void, size, PAGE_NOACCESS, &mut old) != 0
}

pub unsafe fn unprotect(p: *mut u8, size: usize) -> bool {
    if p.is_null() || size == 0 {
        return true;
    }
    let mut old = 0u32;
    VirtualProtect(p as *mut core::ffi::c_void, size, PAGE_READWRITE, &mut old) != 0
}

pub unsafe fn commit(p: *mut u8, size: usize) -> bool {
    if p.is_null() || size == 0 {
        return true;
    }
    !VirtualAlloc(
        p as *mut core::ffi::c_void,
        size,
        MEM_COMMIT,
        PAGE_READWRITE,
    )
    .is_null()
}

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
        let chunk = virt_alloc(4096, true);
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
    let _g = SEG_LOCK.lock();
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
    let raw = virt_alloc(SEGMENT_SIZE, true);
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
