//! Linux-only: `gettid`, THP purge, RISC-V VA probe.

pub use super::unix::*;

use super::unix;

pub fn init() {
    unix::init_page_size();
    unix::set_va_bits(va_bits());
    unix::mix_from_tid(thread_id());
}

pub unsafe fn reuse(_p: *mut u8, _size: usize) {}

#[inline]
pub fn thread_id() -> u32 {
    gettid()
}

#[inline]
pub fn gettid() -> u32 {
    // `SYS_gettid` is the Linux ABI on every arch (glibc and musl).
    unsafe { libc::syscall(libc::SYS_gettid) as u32 }
}

pub unsafe fn madvise_dontneed(p: *mut u8, size: usize) {
    if p.is_null() || size == 0 {
        return;
    }
    crate::stats::purge(size);
    libc::madvise(p as *mut libc::c_void, size, libc::MADV_DONTNEED);
}

/// Usable virtual-address bits (C `unix_detect_virtual_address_bits`).
pub fn va_bits() -> usize {
    #[cfg(target_arch = "riscv64")]
    {
        probe_riscv_va_bits().unwrap_or(48)
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        47
    }
}

#[cfg(target_arch = "riscv64")]
fn probe_riscv_va_bits() -> Option<usize> {
    unsafe {
        let fd = libc::open(
            b"/proc/cpuinfo\0".as_ptr() as *const libc::c_char,
            libc::O_RDONLY,
        );
        if fd < 0 {
            return None;
        }
        let mut buf = [0u8; 4096];
        let n = libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
        libc::close(fd);
        if n <= 0 {
            return None;
        }
        let s = core::str::from_utf8(&buf[..n as usize]).ok()?;
        if s.contains("sv57") {
            return Some(57);
        }
        if s.contains("sv48") {
            return Some(48);
        }
        if s.contains("sv39") {
            return Some(39);
        }
        None
    }
}
