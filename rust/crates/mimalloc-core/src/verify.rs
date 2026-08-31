//! Kani proofs and host tests for pure integer properties.
//!
//! Proofs stay off the OS / SIMD paths (`mmap`, `core::arch`). Install Kani
//! with `cargo install --locked kani-verifier && cargo-kani setup`, then
//! `cargo kani -p mimalloc-core`. Kani is not in nixpkgs; `./tests/kani.sh`
//! no-ops when the verifier is missing.

use crate::page::{decode_addr, encode_addr, padded_need, request_size};
use crate::{align_up, bin, BIN_HUGE};

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
}
