//! Kani proofs and host tests for pure integer properties.
//!
//! Proofs stay off the OS / SIMD paths (`mmap`, `core::arch`). Install Kani
//! with `cargo install --locked kani-verifier && cargo-kani setup`, then
//! `cargo kani -p mimalloc-core` (and `-p vma-core` for the free-list model).
//! Kani is not in nixpkgs; `./tests/kani.sh` no-ops when the verifier is missing.

use crate::page::{
    decode_addr, encode_addr, encode_canary, padded_need, request_size, CANARY_FREED,
};
use crate::{align_up, bin, BIN_HUGE};

/// Synthetic page accounting: after collect, `used + local_len == capacity`.
#[derive(Clone, Copy)]
struct GhostPage {
    capacity: u32,
    used: u32,
    local_len: u32,
}

impl GhostPage {
    fn new(capacity: u32) -> Self {
        Self {
            capacity,
            used: 0,
            local_len: capacity,
        }
    }

    fn inv(&self) -> bool {
        let sum = self.used as u64 + self.local_len as u64;
        sum <= self.capacity as u64
    }

    fn pop(&mut self) -> bool {
        if self.local_len == 0 {
            return false;
        }
        self.local_len -= 1;
        self.used += 1;
        true
    }

    fn push(&mut self) -> bool {
        if self.used == 0 {
            return false;
        }
        self.used -= 1;
        self.local_len += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip_fixed() {
        let keys = [
            (1usize, 2usize),
            (
                0x9E37_79B9_7F4A_7C15u64 as usize,
                0xA076_1D64_78BD_642Fu64 as usize,
            ),
            (usize::MAX, 0),
        ];
        let addrs = [0usize, 1, 8, 16, 4096, usize::MAX / 2, usize::MAX];
        for (k1, k2) in keys {
            for &a in &addrs {
                assert_eq!(decode_addr(k1, k2, encode_addr(k1, k2, a)), a);
            }
        }
    }

    #[test]
    fn align_up_pow2() {
        for align in [1usize, 2, 8, 16, 4096] {
            for x in [0usize, 1, 15, 16, 17, 4095, 4096, 4097] {
                let y = align_up(x, align);
                assert_eq!(y % align, 0);
                assert!(y >= x);
                assert!(y - x < align);
            }
        }
    }

    #[test]
    fn padded_need_covers_request() {
        for size in [0usize, 1, 8, 16, 24, 4096] {
            assert!(padded_need(size) >= request_size(size));
            assert_eq!(padded_need(size), request_size(size) + crate::PADDING_SIZE);
        }
    }

    #[test]
    fn bin_for_size_in_range() {
        for size in 0..=4096 {
            let bin = bin::bin_for_size(size);
            assert!(bin >= 1 && bin <= BIN_HUGE, "size {size} -> bin {bin}");
        }
    }

    #[test]
    fn canary_low_byte_zero_and_not_freed() {
        for enc in [0u32, 1, 0xFF, 0x1FF, 0xABCD_EF01, u32::MAX] {
            let c = encode_canary(enc);
            assert_eq!(c & 0xFF, 0);
            assert_ne!(c, CANARY_FREED);
        }
        assert_eq!(CANARY_FREED & 0x1FF, 0x100);
    }

    #[test]
    fn ghost_page_push_pop() {
        let mut p = GhostPage::new(8);
        assert!(p.inv());
        assert!(p.pop());
        assert!(p.pop());
        assert!(p.push());
        assert_eq!(p.used + p.local_len, p.capacity);
        assert!(p.inv());
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    fn encode_decode_roundtrip() {
        let key1: usize = kani::any();
        let key2: usize = kani::any();
        let addr: usize = kani::any();
        assert_eq!(decode_addr(key1, key2, encode_addr(key1, key2, addr)), addr);
    }

    #[kani::proof]
    fn align_up_no_overflow() {
        let x: usize = kani::any();
        let align: usize = kani::any();
        kani::assume(align.is_power_of_two());
        kani::assume(align >= 1);
        kani::assume(x <= usize::MAX - (align - 1));
        let y = align_up(x, align);
        assert_eq!(y % align, 0);
        assert!(y >= x);
        assert!(y - x < align);
    }

    #[kani::proof]
    fn padded_need_ge_request() {
        let size: usize = kani::any();
        kani::assume(size < usize::MAX - crate::PADDING_SIZE);
        assert!(padded_need(size) >= request_size(size));
    }

    #[kani::proof]
    fn bin_for_size_bounded() {
        let size: usize = kani::any();
        kani::assume(size <= 2048);
        let b = bin::bin_for_size(size);
        assert!(b >= 1);
        assert!(b <= BIN_HUGE);
    }

    #[kani::proof]
    fn bin_for_size_monotonic() {
        let a: usize = kani::any();
        let b: usize = kani::any();
        kani::assume(a <= 512);
        kani::assume(b <= 512);
        kani::assume(a <= b);
        assert!(bin::bin_for_size(a) <= bin::bin_for_size(b));
    }

    #[kani::proof]
    fn canary_low_byte_zero() {
        let enc: u32 = kani::any();
        let c = encode_canary(enc);
        assert_eq!(c & 0xFF, 0);
        assert_ne!(c, CANARY_FREED);
    }

    #[kani::proof]
    #[kani::unwind(10)]
    fn ghost_page_capacity() {
        let cap: u32 = kani::any();
        kani::assume(cap >= 1 && cap <= 8);
        let mut p = GhostPage::new(cap);
        let steps: u8 = kani::any();
        kani::assume(steps <= 8);
        for _ in 0..steps {
            if kani::any() {
                let _ = p.pop();
            } else {
                let _ = p.push();
            }
            assert!(p.inv());
            assert!(p.used + p.local_len == p.capacity);
        }
    }

    #[kani::proof]
    #[kani::unwind(12)]
    fn quarantine_ring_insert_evict_dup() {
        use crate::quarantine::{Insert, Ring};
        let mut r = Ring::<4>::new();
        let cap: usize = kani::any();
        kani::assume(cap >= 4 && cap <= 32);
        let a: usize = kani::any();
        let b: usize = kani::any();
        kani::assume(a != 0 && b != 0 && a != b);
        match r.insert(a, 4, cap) {
            Insert::Held { n, .. } => assert_eq!(n, 0),
            _ => panic!("first insert must hold"),
        }
        match r.insert(a, 4, cap) {
            Insert::Duplicate => {}
            _ => panic!("duplicate"),
        }
        let _ = r.insert(b, 4, cap);
        assert!(r.contains(a) || r.contains(b));
        assert!(!r.contains(0));
    }
}
