//! Linux OS primitives via libc syscall wrappers that do not allocate.

use crate::align_up;
use core::ptr;

pub const PROT_READ: i32 = libc::PROT_READ;
pub const PROT_WRITE: i32 = libc::PROT_WRITE;

static mut OS_PAGE_SIZE: usize = 4096;

pub fn init() {
    unsafe {
        let n = libc::sysconf(libc::_SC_PAGESIZE);
        if n > 0 {
            OS_PAGE_SIZE = n as usize;
        }
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
        p as *mut u8
    }
}

pub unsafe fn munmap(p: *mut u8, size: usize) {
    if !p.is_null() && size != 0 {
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
    let total = size.saturating_add(align);
    if total < size {
        return ptr::null_mut();
    }
    let raw = mmap_anon_prot(total, prot, extra_flags);
    if raw.is_null() {
        return ptr::null_mut();
    }
    let addr = raw as usize;
    let aligned = align_up(addr, align);
    let lead = aligned - addr;
    let trail = total - lead - size;
    if lead >= os {
        munmap(raw, lead);
    }
    if trail >= os {
        munmap((aligned + size) as *mut u8, trail);
    }
    aligned as *mut u8
}

pub unsafe fn madvise_dontneed(p: *mut u8, size: usize) {
    if p.is_null() || size == 0 {
        return;
    }
    libc::madvise(p as *mut libc::c_void, size, libc::MADV_DONTNEED);
}

pub fn set_errno(err: i32) {
    unsafe {
        *libc::__errno_location() = err;
    }
}

pub fn enomem() {
    set_errno(libc::ENOMEM);
}

pub fn einval() {
    set_errno(libc::EINVAL);
}
