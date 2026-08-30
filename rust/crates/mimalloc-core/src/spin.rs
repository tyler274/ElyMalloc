//! Spinlock that never allocates (meta-data, arenas, TLS key creation).
//!
//! Must not call malloc while held. After `fork`, [`SpinLock::force_unlock`]
//! in the child because the holder may not exist.

use core::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock {
    locked: AtomicBool,
}

pub struct SpinGuard<'a> {
    lock: &'a SpinLock,
}

impl SpinLock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn lock(&self) -> SpinGuard<'_> {
        let mut spins = 0u32;
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spins = spins.wrapping_add(1);
            if spins > 100 {
                os_yield();
            } else {
                core::hint::spin_loop();
            }
        }
        SpinGuard { lock: self }
    }

    /// Reset after fork in the child, where the lock may have been held.
    ///
    /// # Safety
    /// Only the `pthread_atfork` child handler; no other thread may be in `lock`.
    pub unsafe fn force_unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

impl Drop for SpinGuard<'_> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}

fn os_yield() {
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        libc::sched_yield();
    }
    #[cfg(target_arch = "wasm32")]
    core::hint::spin_loop();
}
