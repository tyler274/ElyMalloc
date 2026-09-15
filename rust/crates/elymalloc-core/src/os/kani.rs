//! Kani OS / TLS model: region table over a static backing buffer.
//!
//! Not the unix 4 MiB segment cache, ASLR jitter, or real `mmap`/`pthread`.
//! Anonymous maps are page-aligned slots in [`BACKING`]. `protect` updates
//! per-page prot. TLS is two threads × a small key table.
//!
//! `MADV_DONTNEED` zeros the page-aligned interior (stronger than Linux).

use super::{mix_rng, PROT_NONE, PROT_READ, PROT_WRITE};
use crate::align_up;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};

pub const MAP_NORESERVE: i32 = 0;

const PAGE: usize = 4096;
const BACKING: usize = 96 * 1024;
const PAGES: usize = BACKING / PAGE;
const MAX_FREES: usize = 8;
const TLS_THREADS: usize = 2;
const TLS_KEYS: usize = 8;
const TLS_UNSET: usize = usize::MAX;

static mut MEM: [u8; BACKING] = [0; BACKING];
static mut PAGE_MAPPED: [bool; PAGES] = [false; PAGES];
static mut PAGE_PROT: [i32; PAGES] = [0; PAGES];
static mut BUMP: usize = 0;
static mut FREES: [(usize, usize); MAX_FREES] = [(0, 0); MAX_FREES];
static mut NFREE: usize = 0;

static VA_LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);
static ERRNO: AtomicI32 = AtomicI32::new(0);
static TID: AtomicU32 = AtomicU32::new(1);

/// Address of the backing buffer (page-map keys are offsets from this under Kani).
#[inline]
pub fn backing_base() -> usize {
    core::ptr::addr_of!(MEM) as *const u8 as usize
}

pub fn init() {
    mix_rng(0xA076_1D64_78BD_642F);
    set_va_bits(48);
}

#[allow(dead_code)]
pub fn init_page_size() {}

#[inline]
pub fn page_size() -> usize {
    PAGE
}

pub fn abort() -> ! {
    panic!("elymalloc abort");
}

pub fn yield_now() {
    core::hint::spin_loop();
}

#[inline]
pub fn thread_id() -> u32 {
    TID.load(Ordering::Acquire)
}

#[inline]
pub fn gettid() -> u32 {
    thread_id()
}

/// Harness-only: tid `1` or `2` (index 0/1). `0` is reserved like production `CREATE_OWNER`.
pub fn switch_thread(tid: u32) {
    let t = if tid <= 1 { 1 } else { 2 };
    TID.store(t, Ordering::Release);
}

#[allow(dead_code)]
pub fn va_bits() -> usize {
    let lim = VA_LIMIT.load(Ordering::Acquire);
    if lim == usize::MAX {
        usize::BITS as usize
    } else {
        (usize::BITS as usize) - lim.leading_zeros() as usize
    }
}

pub fn set_va_bits(bits: usize) {
    if bits == 0 || bits >= usize::BITS as usize {
        VA_LIMIT.store(usize::MAX, Ordering::Release);
    } else {
        VA_LIMIT.store((1usize << bits) - 1, Ordering::Release);
    }
}

pub unsafe fn reuse(_p: *mut u8, _size: usize) {}

unsafe fn clip_va(p: *mut u8, size: usize, committed: bool) -> *mut u8 {
    if p.is_null() {
        return p;
    }
    let limit = VA_LIMIT.load(Ordering::Acquire);
    let addr = p as usize;
    let last = addr.saturating_add(size.saturating_sub(1));
    if addr > limit || last > limit {
        munmap_ex(p, size, committed);
        return ptr::null_mut();
    }
    p
}

fn tid_index() -> usize {
    if TID.load(Ordering::Acquire) <= 1 {
        0
    } else {
        1
    }
}

fn page_index(p: *const u8) -> Option<usize> {
    let base = backing_base();
    let addr = p as usize;
    if addr < base {
        return None;
    }
    let off = addr - base;
    if off >= BACKING {
        return None;
    }
    Some(off / PAGE)
}

/// Prot of the page containing `p`, if that page is mapped.
pub fn region_prot(p: *const u8) -> Option<i32> {
    let i = page_index(p)?;
    unsafe {
        if PAGE_MAPPED[i] {
            Some(PAGE_PROT[i])
        } else {
            None
        }
    }
}

fn addr_of_page(start: usize) -> usize {
    backing_base() + start * PAGE
}

unsafe fn mark_pages(start: usize, npages: usize, mapped: bool, prot: i32) {
    let mut i = 0;
    while i < npages {
        PAGE_MAPPED[start + i] = mapped;
        PAGE_PROT[start + i] = if mapped { prot } else { 0 };
        i += 1;
    }
}

unsafe fn push_free(start: usize, npages: usize) {
    if npages == 0 || NFREE >= MAX_FREES {
        return;
    }
    FREES[NFREE] = (start, npages);
    NFREE += 1;
}

unsafe fn take_free(npages: usize, align: usize) -> Option<usize> {
    let mut i = 0;
    while i < NFREE {
        let (start, n) = FREES[i];
        if n >= npages && addr_of_page(start) % align == 0 {
            FREES[i] = FREES[NFREE - 1];
            NFREE -= 1;
            FREES[NFREE] = (0, 0);
            if n > npages {
                push_free(start + npages, n - npages);
            }
            return Some(start);
        }
        i += 1;
    }
    None
}

unsafe fn alloc_pages(size: usize, align: usize, prot: i32) -> *mut u8 {
    if size == 0 || align == 0 || !align.is_power_of_two() {
        return ptr::null_mut();
    }
    let size = align_up(size, PAGE);
    let align = align.max(PAGE);
    let npages = size / PAGE;
    if npages == 0 || npages > PAGES {
        return ptr::null_mut();
    }
    let start = if let Some(s) = take_free(npages, align) {
        s
    } else {
        let addr0 = backing_base().saturating_add(BUMP * PAGE);
        let aligned = crate::align_up(addr0, align);
        if aligned < backing_base() {
            return ptr::null_mut();
        }
        let start = (aligned - backing_base()) / PAGE;
        if start.saturating_add(npages) > PAGES {
            return ptr::null_mut();
        }
        BUMP = start + npages;
        start
    };
    mark_pages(start, npages, true, prot);
    let p = (core::ptr::addr_of_mut!(MEM) as *mut u8).add(start * PAGE);
    let committed = (prot & (PROT_READ | PROT_WRITE)) != 0;
    crate::stats::mmap_map(size, committed);
    clip_va(p, size, committed)
}

pub unsafe fn mmap_anon(size: usize) -> *mut u8 {
    mmap_anon_prot(size, PROT_READ | PROT_WRITE, 0)
}

unsafe fn mmap_anon_prot(size: usize, prot: i32, _extra_flags: i32) -> *mut u8 {
    alloc_pages(size, PAGE, prot)
}

pub unsafe fn munmap(p: *mut u8, size: usize) {
    munmap_ex(p, size, true);
}

pub unsafe fn munmap_ex(p: *mut u8, size: usize, committed: bool) {
    if p.is_null() || size == 0 {
        return;
    }
    let Some(start) = page_index(p) else {
        return;
    };
    let size = align_up(size, PAGE);
    let npages = size / PAGE;
    if start + npages > PAGES {
        return;
    }
    mark_pages(start, npages, false, 0);
    if BUMP == start + npages {
        BUMP = start;
    } else {
        push_free(start, npages);
    }
    crate::stats::mmap_unmap(size, committed);
}

pub unsafe fn mmap_aligned(size: usize, align: usize) -> *mut u8 {
    mmap_aligned_prot(size, align, PROT_READ | PROT_WRITE, 0)
}

pub unsafe fn mmap_aligned_prot(size: usize, align: usize, prot: i32, _extra_flags: i32) -> *mut u8 {
    alloc_pages(size, align, prot)
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

unsafe fn set_prot_range(p: *mut u8, size: usize, prot: i32) -> bool {
    if p.is_null() || size == 0 {
        return true;
    }
    let Some(start) = page_index(p) else {
        return false;
    };
    let npages = align_up(size, PAGE) / PAGE;
    if start + npages > PAGES {
        return false;
    }
    let mut i = 0;
    while i < npages {
        if !PAGE_MAPPED[start + i] {
            return false;
        }
        i += 1;
    }
    i = 0;
    while i < npages {
        PAGE_PROT[start + i] = prot;
        i += 1;
    }
    true
}

pub unsafe fn protect(p: *mut u8, size: usize) -> bool {
    set_prot_range(p, size, PROT_NONE)
}

pub unsafe fn unprotect(p: *mut u8, size: usize) -> bool {
    set_prot_range(p, size, PROT_READ | PROT_WRITE)
}

pub unsafe fn commit(p: *mut u8, size: usize) -> bool {
    unprotect(p, size)
}

#[allow(dead_code)]
pub unsafe fn force_unlock() {}

/// Process-wide TLS key. Same methods as the pthread / `TlsAlloc` backend.
pub struct TlsSlot {
    key: AtomicUsize,
}

static mut TLS_SLOTS: [[*mut c_void; TLS_KEYS]; TLS_THREADS] =
    [[ptr::null_mut(); TLS_KEYS]; TLS_THREADS];
static mut TLS_DTORS: [Option<unsafe extern "C" fn(*mut c_void)>; TLS_KEYS] = [None; TLS_KEYS];
static TLS_NEXT: AtomicUsize = AtomicUsize::new(0);

impl TlsSlot {
    pub const fn new() -> Self {
        Self {
            key: AtomicUsize::new(TLS_UNSET),
        }
    }

    #[inline]
    pub fn is_ready(&self) -> bool {
        self.key.load(Ordering::Acquire) != TLS_UNSET
    }

    pub fn ensure(&self, dtor: Option<unsafe extern "C" fn(*mut c_void)>) {
        if self.is_ready() {
            return;
        }
        let k = TLS_NEXT.fetch_add(1, Ordering::AcqRel);
        if k >= TLS_KEYS {
            abort();
        }
        unsafe {
            TLS_DTORS[k] = dtor;
        }
        let _ = self
            .key
            .compare_exchange(TLS_UNSET, k, Ordering::AcqRel, Ordering::Acquire);
    }

    #[inline]
    pub unsafe fn get(&self) -> *mut c_void {
        let k = self.key.load(Ordering::Acquire);
        if k == TLS_UNSET || k >= TLS_KEYS {
            return ptr::null_mut();
        }
        TLS_SLOTS[tid_index()][k]
    }

    #[inline]
    pub unsafe fn get_non_null<T>(&self) -> Option<NonNull<T>> {
        NonNull::new(self.get().cast())
    }

    #[inline]
    pub unsafe fn set(&self, p: *mut c_void) {
        let k = self.key.load(Ordering::Acquire);
        if k == TLS_UNSET || k >= TLS_KEYS {
            return;
        }
        TLS_SLOTS[tid_index()][k] = p;
    }

    #[inline]
    pub unsafe fn set_non_null<T>(&self, p: Option<NonNull<T>>) {
        self.set(p.map(|n| n.as_ptr().cast()).unwrap_or(ptr::null_mut()));
    }

    #[inline]
    #[allow(dead_code)]
    pub fn raw_key(&self) -> usize {
        self.key.load(Ordering::Acquire)
    }
}

/// Run dtors for the current tid and clear its slots.
pub fn thread_exit() {
    let t = tid_index();
    let mut k = 0;
    while k < TLS_KEYS {
        unsafe {
            let p = TLS_SLOTS[t][k];
            TLS_SLOTS[t][k] = ptr::null_mut();
            if let Some(dtor) = TLS_DTORS[k] {
                if !p.is_null() {
                    dtor(p);
                }
            }
        }
        k += 1;
    }
}
