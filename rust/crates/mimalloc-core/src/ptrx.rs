//! Strict-provenance helpers.
//!
//! Encoded free-list next pointers are integers, not live references. Decode
//! with [`with_exposed`] rather than `as *mut T`, and compare addresses with
//! [`addr`] instead of `as usize` (which exposes provenance).
//!
//! Invariant: a decoded next pointer is either null (the page address as
//! sentinel) or a block start inside that page. Out-of-range decode is
//! treated as heap corruption.

#![allow(dead_code)]

/// Address bits only; does not expose provenance (`pointer::addr`).
#[inline]
pub fn addr<T>(p: *const T) -> usize {
    p.addr()
}

/// Address bits of a `*mut` (same as [`addr`]).
#[inline]
pub fn addr_mut<T>(p: *mut T) -> usize {
    p.addr()
}

/// Rebuild a pointer from an integer address (exposed provenance).
#[inline]
pub fn with_exposed<T>(addr: usize) -> *mut T {
    core::ptr::with_exposed_provenance_mut(addr)
}

#[inline]
pub fn with_exposed_const<T>(addr: usize) -> *const T {
    core::ptr::with_exposed_provenance(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addr_matches_integer_bits() {
        let x = 0xABu8;
        let p = &x as *const u8;
        assert_eq!(addr(p), p as usize);
    }

    #[test]
    fn exposed_roundtrip_same_addr() {
        let x = 1u32;
        let p = &x as *const u32 as *mut u32;
        let q: *mut u32 = with_exposed(addr_mut(p));
        assert_eq!(addr_mut(p), addr_mut(q));
    }
}
