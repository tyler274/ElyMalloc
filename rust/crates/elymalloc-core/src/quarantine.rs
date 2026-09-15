//! Delayed-free quarantine (Scudo / Graphene default). Off unless
//! [`OPTION_QUARANTINE`] is non-zero (value is KiB; [`crate::options::get_size`]
//! converts to bytes). Double-free of a quarantined pointer is still `EAGAIN`.
//!
//! This is a rewrite-only option (C `mi_option_e` ends at `_mi_option_last`).
//! Default 0 so compiler-preload / world / browsers stay comparable to C.

use core::ptr;

/// `mi_option` index: quarantine cap in KiB (0 = off).
pub const OPTION_QUARANTINE: i32 = 47;
/// `mi_option` index: fill user bytes with 0 on free before quarantine/page push.
pub const OPTION_ZERO_ON_FREE: i32 = 48;

/// One delayed-free slot (`ptr` is a user pointer address; 0 is empty).
#[derive(Clone, Copy, Debug, Default)]
pub struct Slot {
    pub ptr: usize,
    pub size: usize,
}

/// Bounded FIFO ring. `N` is the slot cap (runtime uses 32; Kani uses 4).
pub struct Ring<const N: usize> {
    slots: [Slot; N],
    oldest: usize,
    count: usize,
    bytes: usize,
}

impl<const N: usize> Ring<N> {
    pub const fn new() -> Self {
        Self {
            slots: [Slot { ptr: 0, size: 0 }; N],
            oldest: 0,
            count: 0,
            bytes: 0,
        }
    }

    pub fn contains(&self, ptr: usize) -> bool {
        if ptr == 0 || self.count == 0 {
            return false;
        }
        let mut i = self.oldest;
        for _ in 0..self.count {
            if self.slots[i].ptr == ptr {
                return true;
            }
            i += 1;
            if i == N {
                i = 0;
            }
        }
        false
    }

    pub fn pop_oldest(&mut self) -> Option<Slot> {
        if self.count == 0 {
            return None;
        }
        let s = self.slots[self.oldest];
        self.slots[self.oldest] = Slot { ptr: 0, size: 0 };
        self.oldest += 1;
        if self.oldest == N {
            self.oldest = 0;
        }
        self.count -= 1;
        self.bytes = self.bytes.saturating_sub(s.size);
        Some(s)
    }

    /// Insert `ptr` of `size` bytes under a byte cap. Evicts oldest until it
    /// fits (or returns [`Insert::Bypass`] if `size` itself exceeds the cap).
    pub fn insert(&mut self, ptr: usize, size: usize, cap: usize) -> Insert<N> {
        if cap == 0 || size == 0 || size > cap || ptr == 0 {
            return Insert::Bypass;
        }
        if self.contains(ptr) {
            return Insert::Duplicate;
        }
        let mut evicted = [Slot { ptr: 0, size: 0 }; N];
        let mut n = 0usize;
        while (self.bytes.saturating_add(size) > cap || self.count == N) && self.count > 0 {
            if let Some(s) = self.pop_oldest() {
                evicted[n] = s;
                n += 1;
            }
        }
        if self.bytes.saturating_add(size) > cap || self.count == N {
            return Insert::BypassWith { evicted, n };
        }
        let mut idx = self.oldest + self.count;
        if idx >= N {
            idx -= N;
        }
        self.slots[idx] = Slot { ptr, size };
        self.count += 1;
        self.bytes += size;
        Insert::Held { evicted, n }
    }

    pub fn drain_into(&mut self, out: &mut [Slot]) -> usize {
        let mut n = 0usize;
        while n < out.len() {
            match self.pop_oldest() {
                Some(s) => {
                    out[n] = s;
                    n += 1;
                }
                None => break,
            }
        }
        n
    }
}

/// Result of [`Ring::insert`].
pub enum Insert<const N: usize> {
    /// Cap is 0, size is 0, or the block is larger than the cap: free now.
    Bypass,
    /// Evicted some slots while trying to fit, then still could not insert.
    BypassWith { evicted: [Slot; N], n: usize },
    /// Pointer is already in the ring (double-free).
    Duplicate,
    /// Inserted; `evicted[..n]` must be returned to the page.
    Held { evicted: [Slot; N], n: usize },
}

const TLS_SLOTS: usize = 32;

#[cfg(not(target_arch = "wasm32"))]
mod tls_ring {
    use super::*;
    use crate::os::TlsSlot;
    use core::ptr::NonNull;

    static KEY: TlsSlot = TlsSlot::new();

    unsafe fn ring() -> *mut Ring<TLS_SLOTS> {
        KEY.ensure(None);
        if let Some(p) = KEY.get_non_null::<Ring<TLS_SLOTS>>() {
            return p.as_ptr();
        }
        let Some(map) = crate::os::Mapping::anon(core::mem::size_of::<Ring<TLS_SLOTS>>()) else {
            return ptr::null_mut();
        };
        let raw = map.leak() as *mut Ring<TLS_SLOTS>;
        ptr::write(raw, Ring::new());
        KEY.set_non_null(NonNull::new(raw));
        raw
    }

    pub unsafe fn with_ring<R>(f: impl FnOnce(&mut Ring<TLS_SLOTS>) -> R) -> Option<R> {
        let r = ring();
        if r.is_null() {
            None
        } else {
            Some(f(&mut *r))
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod tls_ring {
    use super::*;

    static mut RING: Ring<TLS_SLOTS> = Ring::new();

    pub unsafe fn with_ring<R>(f: impl FnOnce(&mut Ring<TLS_SLOTS>) -> R) -> Option<R> {
        Some(f(&mut *ptr::addr_of_mut!(RING)))
    }
}

/// Cap in bytes from the option (0 = quarantine off).
#[inline]
pub fn cap_bytes() -> usize {
    crate::options::get_size(OPTION_QUARANTINE)
}

/// True when [`OPTION_ZERO_ON_FREE`] is non-zero.
#[inline]
pub fn zero_on_free() -> bool {
    crate::options::is_enabled(OPTION_ZERO_ON_FREE)
}

/// Push `p` into this thread's ring. The caller recycles evicted pointers.
pub unsafe fn push(p: *mut u8, size: usize) -> Insert<TLS_SLOTS> {
    let cap = cap_bytes();
    if cap == 0 || p.is_null() {
        return Insert::Bypass;
    }
    match tls_ring::with_ring(|r| r.insert(p as usize, size, cap)) {
        Some(ins) => ins,
        None => Insert::Bypass,
    }
}

/// True if this thread's ring already holds `p` (Graphene double-free).
pub unsafe fn contains(p: *mut u8) -> bool {
    if p.is_null() {
        return false;
    }
    tls_ring::with_ring(|r| r.contains(p as usize)).unwrap_or(false)
}

/// Remove every quarantined pointer. The caller must recycle them.
pub unsafe fn drain(out: &mut [Slot]) -> usize {
    tls_ring::with_ring(|r| r.drain_into(out)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_evict_duplicate() {
        let mut r = Ring::<4>::new();
        match r.insert(0x1000, 8, 16) {
            Insert::Held { n, .. } => assert_eq!(n, 0),
            _ => panic!("held"),
        }
        match r.insert(0x1000, 8, 16) {
            Insert::Duplicate => {}
            _ => panic!("dup"),
        }
        match r.insert(0x2000, 8, 16) {
            Insert::Held { n, .. } => assert_eq!(n, 0),
            _ => panic!("held2"),
        }
        // 8+8+8 > 16 → evict oldest
        match r.insert(0x3000, 8, 16) {
            Insert::Held { n, evicted } => {
                assert_eq!(n, 1);
                assert_eq!(evicted[0].ptr, 0x1000);
            }
            _ => panic!("evict"),
        }
        assert!(r.contains(0x2000));
        assert!(r.contains(0x3000));
        assert!(!r.contains(0x1000));
    }

    #[test]
    fn too_big_bypasses() {
        let mut r = Ring::<4>::new();
        match r.insert(0x10, 32, 16) {
            Insert::Bypass => {}
            _ => panic!("bypass"),
        }
        match r.insert(0x10, 8, 0) {
            Insert::Bypass => {}
            _ => panic!("cap0"),
        }
    }
}
