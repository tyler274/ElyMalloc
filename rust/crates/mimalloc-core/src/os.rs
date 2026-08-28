//! Linux OS primitives via libc syscall wrappers that do not allocate.

use crate::align_up;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

pub const PROT_READ: i32 = libc::PROT_READ;
pub const PROT_WRITE: i32 = libc::PROT_WRITE;
pub const PROT_NONE: i32 = libc::PROT_NONE;

static mut OS_PAGE_SIZE: usize = 4096;
static RNG: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);

pub fn init() {
    unsafe {
        let n = libc::sysconf(libc::_SC_PAGESIZE);
        if n > 0 {
            OS_PAGE_SIZE = n as usize;
        }
        let mix = (gettid() as u64)
            .wrapping_mul(0xA076_1D64_78BD_642F)
            .wrapping_add(OS_PAGE_SIZE as u64);
        RNG.fetch_xor(mix, Ordering::Relaxed);
    }
}

/// Cheap xorshift for ASLR-style jitter (not a cryptographic RNG).
pub fn random_u64() -> u64 {
    let mut x = RNG.load(Ordering::Relaxed);
    if x == 0 {
        x = 0x9E37_79B9_7F4A_7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG.store(x, Ordering::Relaxed);
    x
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
    unsafe { libc::syscall(libc::SYS_gettid) as u32 }
}

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
    if !p.is_null() && size != 0 {
        crate::stats::mmap_unmap(size, committed);
        libc::munmap(p as *mut libc::c_void, size);
    }
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
        munmap_ex(raw, lead, committed);
    }
    if trail >= os {
        munmap_ex((aligned + size) as *mut u8, trail, committed);
    }
    aligned as *mut u8
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
        *libc::__errno_location() = err;
    }
}

pub fn enomem() {
    set_errno(libc::ENOMEM);
    crate::hooks::error(libc::ENOMEM);
}

pub fn einval() {
    set_errno(libc::EINVAL);
    crate::hooks::error(libc::EINVAL);
}

pub fn eagain() {
    set_errno(libc::EAGAIN);
    crate::hooks::error(libc::EAGAIN);
}

/// Report `EFAULT` without aborting (padding overflow / double-free).
pub fn efault_report() {
    set_errno(libc::EFAULT);
    crate::hooks::error(libc::EFAULT);
}

pub fn efault() -> ! {
    efault_report();
    abort();
}

pub unsafe fn protect(p: *mut u8, size: usize) -> bool {
    if p.is_null() || size == 0 {
        return true;
    }
    libc::mprotect(p as *mut libc::c_void, size, PROT_NONE) == 0
}

pub unsafe fn unprotect(p: *mut u8, size: usize) -> bool {
    if p.is_null() || size == 0 {
        return true;
    }
    libc::mprotect(p as *mut libc::c_void, size, PROT_READ | PROT_WRITE) == 0
}
