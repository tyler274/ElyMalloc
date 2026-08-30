//! Linux OS primitives via libc syscall wrappers that do not allocate.

use super::{mix_rng, random_u64, PROT_NONE, PROT_READ, PROT_WRITE};
use crate::align_up;
use core::ptr;

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
        *errno_location() = err;
    }
}

#[inline]
fn errno_location() -> *mut i32 {
    unsafe { libc::__errno_location() }
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

pub unsafe fn commit(p: *mut u8, size: usize) -> bool {
    unprotect(p, size)
}
