//! OS memory primitives behind a small safe API.
//!
//! `mimalloc-core` is `no_std` and must not call `std` (it allocates). `unsafe`
//! stays in the backend (`mmap` / `VirtualAlloc` / `memory.grow`). Callers use
//! [`Mapping`], [`thread_id`], [`protect`], and [`purge`].
//!
//! Wrappers must not allocate (no libc malloc). Alignment over-maps then
//! trims lead/trail on Unix. Guard pages use `PROT_NONE` / `PAGE_NOACCESS`.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};

/// POSIX `ENOMEM` (same number on Linux and in our wasm errno cell).
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;
pub const EAGAIN: i32 = 11;
pub const EFAULT: i32 = 14;

pub const PROT_NONE: i32 = 0;
pub const PROT_READ: i32 = 1;
pub const PROT_WRITE: i32 = 2;

static RNG: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);

/// Cheap xorshift for ASLR-style jitter (not a cryptographic RNG).
#[allow(dead_code)]
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

pub(crate) fn mix_rng(mix: u64) {
    RNG.fetch_xor(mix, Ordering::Relaxed);
}

pub fn enomem() {
    set_errno(ENOMEM);
    crate::hooks::error(ENOMEM);
}

pub fn einval() {
    set_errno(EINVAL);
    crate::hooks::error(EINVAL);
}

pub fn eagain() {
    set_errno(EAGAIN);
    crate::hooks::error(EAGAIN);
}

/// Report `EFAULT` without aborting (padding overflow / double-free).
pub fn efault_report() {
    set_errno(EFAULT);
    crate::hooks::error(EFAULT);
}

pub fn efault() -> ! {
    efault_report();
    abort();
}

/// Owned anonymous mapping. `Drop` unmaps unless [`Mapping::leak`] hands the
/// region to the page map / a page.
#[allow(dead_code)]
pub struct Mapping {
    ptr: NonNull<u8>,
    len: usize,
    committed: bool,
    leaked: bool,
}

#[allow(dead_code)]
impl Mapping {
    /// Reserve+commit `size` bytes (rounded up by the backend).
    pub fn anon(size: usize) -> Option<Self> {
        if size == 0 {
            return None;
        }
        let p = unsafe { mmap_anon(size) };
        NonNull::new(p).map(|ptr| Self {
            ptr,
            len: size,
            committed: true,
            leaked: false,
        })
    }

    /// `size` bytes aligned to `align` (both powers of two / page multiples).
    pub fn aligned(size: usize, align: usize) -> Option<Self> {
        if size == 0 || align == 0 {
            return None;
        }
        let p = unsafe { mmap_aligned(size, align) };
        NonNull::new(p).map(|ptr| Self {
            ptr,
            len: size,
            committed: true,
            leaked: false,
        })
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Transfer ownership to the page map. The caller must later [`unmap`].
    pub fn leak(mut self) -> *mut u8 {
        self.leaked = true;
        self.ptr.as_ptr()
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        if !self.leaked {
            unsafe {
                munmap_ex(self.ptr.as_ptr(), self.len, self.committed);
            }
        }
    }
}

/// `MADV_DONTNEED` / Apple reusable / no-op on wasm. Size 0 or null is a no-op.
pub unsafe fn purge(p: *mut u8, size: usize) {
    madvise_dontneed(p, size);
}

/// C `_mi_os_minimal_purge_size`. `allow_thp==2` (FULL) uses 2 MiB so
/// `MADV_DONTNEED` does not split transparent huge pages (Linux only).
pub fn min_purge_size() -> usize {
    let explicit = crate::options::get_size(44);
    if explicit != 0 {
        crate::align_up(explicit, page_size())
    } else if cfg!(target_os = "linux") && crate::options::get(43) == 2 {
        2 * 1024 * 1024
    } else {
        page_size()
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(not(target_arch = "wasm32"))]
mod tls_slot;
#[cfg(not(target_arch = "wasm32"))]
pub use tls_slot::TlsSlot;
