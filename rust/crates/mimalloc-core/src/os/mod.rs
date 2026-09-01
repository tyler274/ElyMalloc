//! OS memory primitives. Linux uses `mmap`; wasm32 uses `memory.grow`.
//!
//! Wrappers must not allocate (no libc malloc). Alignment over-maps then
//! trims lead/trail and optionally jitters the start (C `MI_SECURE` ASLR).
//! Guard pages use `mprotect(PROT_NONE)`. Errors set `errno` and invoke
//! [`crate::hooks::error`].

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

#[cfg(not(target_arch = "wasm32"))]
mod linux;
#[cfg(not(target_arch = "wasm32"))]
pub use linux::*;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

/// C `_mi_os_minimal_purge_size`. `allow_thp==2` (FULL) uses 2 MiB so
/// `MADV_DONTNEED` does not split transparent huge pages.
pub fn min_purge_size() -> usize {
    let explicit = crate::options::get_size(44);
    if explicit != 0 {
        crate::align_up(explicit, page_size())
    } else if crate::options::get(43) == 2 {
        2 * 1024 * 1024
    } else {
        page_size()
    }
}
