//! SIMD lane-twin proofs (integer nests ≡ `write_bytes` / copy / eq).

#[cfg(test)]
mod tests {
    use crate::mem;
    use core::ptr;

    #[test]
    fn sse2_lanes_match_write_bytes() {
        let mut a = [0u8; 200];
        let mut b = [0u8; 200];
        unsafe {
            mem::fill_sse2_lanes(a.as_mut_ptr(), 0xA5, 200);
            ptr::write_bytes(b.as_mut_ptr(), 0xA5, 200);
        }
        assert_eq!(a, b);
    }

    #[test]
    fn avx512_lanes_match_prefix() {
        let mut a = [0u8; 200];
        let mut b = [0u8; 200];
        unsafe {
            let off = mem::fill_avx512_lanes(a.as_mut_ptr(), 0x3C, 200);
            ptr::write_bytes(b.as_mut_ptr(), 0x3C, off);
            assert_eq!(off, 192);
            assert_eq!(&a[..off], &b[..off]);
            assert!(mem::eq_filled_avx512_lanes(a.as_ptr(), 0x3C, off).is_ok());
        }
    }

    #[test]
    fn copy_lanes_then_scalar_tail() {
        let mut src = [0u8; 200];
        let mut dst = [0u8; 200];
        for (i, x) in src.iter_mut().enumerate() {
            *x = (i % 251) as u8;
        }
        unsafe {
            let off = mem::copy_avx512_lanes(dst.as_mut_ptr(), src.as_ptr(), 200);
            ptr::copy_nonoverlapping(src.as_ptr().add(off), dst.as_mut_ptr().add(off), 200 - off);
        }
        assert_eq!(dst, src);
    }

    #[test]
    fn fill_eq_matches_lanes() {
        let mut b = [0u8; 128];
        unsafe {
            mem::fill(b.as_mut_ptr(), 0x11, 128);
            assert!(mem::eq_filled(b.as_ptr(), 0x11, 128));
            assert!(mem::eq_filled_sse2_lanes(b.as_ptr(), 0x11, 128));
            b[64] = 0;
            assert!(!mem::eq_filled(b.as_ptr(), 0x11, 128));
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use crate::mem;
    use core::ptr;

    #[kani::proof]
    #[kani::unwind(18)]
    fn fill_sse2_lanes_eq_write_bytes() {
        let n: usize = kani::any();
        kani::assume(n <= 16);
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        let byte: u8 = kani::any();
        unsafe {
            mem::fill_sse2_lanes(a.as_mut_ptr(), byte, n);
            ptr::write_bytes(b.as_mut_ptr(), byte, n);
        }
        let mut i = 0usize;
        while i < n {
            assert_eq!(a[i], b[i]);
            i += 1;
        }
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn fill_avx512_lanes_eq_write_bytes() {
        let n: usize = kani::any();
        kani::assume(n <= 64);
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        let byte: u8 = kani::any();
        unsafe {
            let off = mem::fill_avx512_lanes(a.as_mut_ptr(), byte, n);
            ptr::write_bytes(b.as_mut_ptr(), byte, off);
            assert!(off <= n);
            assert_eq!(off, n - (n % 64));
            if off > 0 {
                assert_eq!(a[0], byte);
                assert_eq!(a[off - 1], byte);
            }
        }
    }

    #[kani::proof]
    #[kani::unwind(18)]
    fn eq_filled_sse2_lanes() {
        let n: usize = kani::any();
        kani::assume(n <= 16);
        let mut a = [0u8; 16];
        let byte: u8 = kani::any();
        unsafe {
            ptr::write_bytes(a.as_mut_ptr(), byte, n);
            assert!(mem::eq_filled_sse2_lanes(a.as_ptr(), byte, n));
        }
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn copy_avx512_then_scalar() {
        let n: usize = 64;
        let mut src = [0u8; 64];
        let mut dst = [0u8; 64];
        let b: u8 = kani::any();
        unsafe {
            ptr::write_bytes(src.as_mut_ptr(), b, n);
            let off = mem::copy_avx512_lanes(dst.as_mut_ptr(), src.as_ptr(), n);
            ptr::copy_nonoverlapping(src.as_ptr().add(off), dst.as_mut_ptr().add(off), n - off);
            assert_eq!(off, 64);
            assert_eq!(dst[0], b);
            assert_eq!(dst[63], b);
        }
    }
}
