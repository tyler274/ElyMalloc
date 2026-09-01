//! Thread-local slot that does not allocate (pthread keys / `TlsAlloc`).
//!
//! Do not use `#[thread_local]` for the heap pointer: glibc TLS init mallocs.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

const UNSET: usize = usize::MAX;

/// Process-wide TLS key. `T` is stored as a raw pointer (theap / subproc).
pub struct TlsSlot {
    key: AtomicUsize,
}

impl TlsSlot {
    pub const fn new() -> Self {
        Self {
            key: AtomicUsize::new(UNSET),
        }
    }

    #[inline]
    pub fn is_ready(&self) -> bool {
        self.key.load(Ordering::Acquire) != UNSET
    }

    /// Create the OS key once. `dtor` runs on thread exit (Unix); Windows
    /// callers must invoke the destructor from `thread_done`.
    pub fn ensure(&self, dtor: Option<unsafe extern "C" fn(*mut c_void)>) {
        if self.is_ready() {
            return;
        }
        self.ensure_slow(dtor);
    }

    #[inline]
    pub unsafe fn get(&self) -> *mut c_void {
        let k = self.key.load(Ordering::Acquire);
        if k == UNSET {
            return ptr::null_mut();
        }
        get_specific(k)
    }

    #[inline]
    pub unsafe fn set(&self, p: *mut c_void) {
        let k = self.key.load(Ordering::Acquire);
        if k == UNSET {
            return;
        }
        set_specific(k, p);
    }

    #[inline]
    #[allow(dead_code)]
    pub fn raw_key(&self) -> usize {
        self.key.load(Ordering::Acquire)
    }
}

#[cfg(unix)]
impl TlsSlot {
    fn ensure_slow(&self, dtor: Option<unsafe extern "C" fn(*mut c_void)>) {
        let mut key: libc::pthread_key_t = 0;
        unsafe {
            if libc::pthread_key_create(&mut key, dtor) != 0 {
                crate::os::abort();
            }
        }
        match self
            .key
            .compare_exchange(UNSET, key as usize, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {}
            Err(_) => unsafe {
                libc::pthread_key_delete(key);
            },
        }
    }
}

#[cfg(unix)]
unsafe fn get_specific(k: usize) -> *mut c_void {
    libc::pthread_getspecific(k as libc::pthread_key_t)
}

#[cfg(unix)]
unsafe fn set_specific(k: usize, p: *mut c_void) {
    libc::pthread_setspecific(k as libc::pthread_key_t, p);
}

#[cfg(windows)]
impl TlsSlot {
    fn ensure_slow(&self, _dtor: Option<unsafe extern "C" fn(*mut c_void)>) {
        unsafe {
            let idx = windows_sys::Win32::System::Threading::TlsAlloc();
            if idx == windows_sys::Win32::System::Threading::TLS_OUT_OF_INDEXES {
                crate::os::abort();
            }
            if self
                .key
                .compare_exchange(UNSET, idx as usize, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                windows_sys::Win32::System::Threading::TlsFree(idx);
            }
        }
    }
}

#[cfg(windows)]
unsafe fn get_specific(k: usize) -> *mut c_void {
    windows_sys::Win32::System::Threading::TlsGetValue(k as u32)
}

#[cfg(windows)]
unsafe fn set_specific(k: usize, p: *mut c_void) {
    let _ = windows_sys::Win32::System::Threading::TlsSetValue(k as u32, p);
}
