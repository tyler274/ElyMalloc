//! macOS: pthread tid, `MADV_FREE_REUSABLE` purge.

pub use super::unix::*;

use super::unix;

pub fn init() {
    unix::init_page_size();
    unix::set_va_bits(va_bits());
    unix::mix_from_tid(thread_id());
}

#[inline]
pub fn thread_id() -> u32 {
    gettid()
}

#[inline]
pub fn gettid() -> u32 {
    unsafe {
        let mut tid: u64 = 0;
        // NULL thread => current (Darwin `pthread_threadid_np`).
        libc::pthread_threadid_np(0 as libc::pthread_t, &mut tid);
        tid as u32
    }
}

pub unsafe fn madvise_dontneed(p: *mut u8, size: usize) {
    if p.is_null() || size == 0 {
        return;
    }
    crate::stats::purge(size);
    // C #1097: `MADV_FREE_REUSABLE` keeps RSS accounting honest on Darwin.
    #[cfg(target_os = "macos")]
    {
        const MADV_FREE_REUSABLE: i32 = 8;
        if libc::madvise(p as *mut libc::c_void, size, MADV_FREE_REUSABLE) != 0 {
            libc::madvise(p as *mut libc::c_void, size, libc::MADV_DONTNEED);
        }
    }
}

pub fn va_bits() -> usize {
    47
}

/// Undo [`madvise_dontneed`] (`MADV_FREE_REUSE`, C #1097).
pub unsafe fn reuse(p: *mut u8, size: usize) {
    if p.is_null() || size == 0 {
        return;
    }
    const MADV_FREE_REUSE: i32 = 9;
    let _ = libc::madvise(p as *mut libc::c_void, size, MADV_FREE_REUSE);
}
