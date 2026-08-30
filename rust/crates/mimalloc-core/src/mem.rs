//! Byte fill / copy / compare with SIMD on common host targets.
//!
//! Small sizes stay on SSE2 / NEON. Lengths of at least [`AVX512_MIN`] use
//! AVX-512 (`AVX512F` + `AVX512BW`, 64-byte ZMM) when the CPU and OS enable it
//! (Zen 5, Ice Lake+, and compile-time `target-cpu` with those features).
//! Detection is cached CPUID + `XCR0`; Kani/wasm keep the scalar path.

use core::ptr;

/// Prefer a full ZMM store over SSE2 once the run is at least this long.
#[cfg(all(target_arch = "x86_64", not(kani)))]
const AVX512_MIN: usize = 64;

#[inline]
pub unsafe fn fill(p: *mut u8, byte: u8, n: usize) {
    if p.is_null() || n == 0 {
        return;
    }
    #[cfg(all(target_arch = "x86_64", not(kani)))]
    fill_x86(p, byte, n);
    #[cfg(all(target_arch = "aarch64", not(kani)))]
    fill_neon(p, byte, n);
    #[cfg(not(any(
        all(target_arch = "x86_64", not(kani)),
        all(target_arch = "aarch64", not(kani))
    )))]
    ptr::write_bytes(p, byte, n);
}

#[inline]
pub unsafe fn copy(dst: *mut u8, src: *const u8, n: usize) {
    if n == 0 || dst.is_null() || src.is_null() {
        return;
    }
    #[cfg(all(target_arch = "x86_64", not(kani)))]
    copy_x86(dst, src, n);
    #[cfg(not(all(target_arch = "x86_64", not(kani))))]
    ptr::copy_nonoverlapping(src, dst, n);
}

/// True if `p[0..n]` is filled with `byte`.
#[inline]
pub unsafe fn eq_filled(p: *const u8, byte: u8, n: usize) -> bool {
    if n == 0 {
        return true;
    }
    if p.is_null() {
        return false;
    }
    #[cfg(all(target_arch = "x86_64", not(kani)))]
    {
        return eq_filled_x86(p, byte, n);
    }
    #[cfg(all(target_arch = "aarch64", not(kani)))]
    {
        return eq_filled_neon(p, byte, n);
    }
    #[cfg(not(any(
        all(target_arch = "x86_64", not(kani)),
        all(target_arch = "aarch64", not(kani))
    )))]
    {
        for i in 0..n {
            if *p.add(i) != byte {
                return false;
            }
        }
        true
    }
}

#[cfg(all(target_arch = "x86_64", not(kani)))]
#[inline]
unsafe fn fill_x86(p: *mut u8, byte: u8, n: usize) {
    let mut off = 0;
    if n >= AVX512_MIN && has_avx512bw() {
        off = fill_avx512(p, byte, n);
    }
    fill_sse2(p.add(off), byte, n - off);
}

#[cfg(all(target_arch = "x86_64", not(kani)))]
#[inline]
unsafe fn copy_x86(dst: *mut u8, src: *const u8, n: usize) {
    let mut off = 0;
    if n >= AVX512_MIN && has_avx512bw() {
        off = copy_avx512(dst, src, n);
    }
    ptr::copy_nonoverlapping(src.add(off), dst.add(off), n - off);
}

#[cfg(all(target_arch = "x86_64", not(kani)))]
#[inline]
unsafe fn eq_filled_x86(p: *const u8, byte: u8, n: usize) -> bool {
    if n >= AVX512_MIN && has_avx512bw() {
        match eq_filled_avx512(p, byte, n) {
            Ok(off) => return eq_filled_sse2(p.add(off), byte, n - off),
            Err(()) => return false,
        }
    }
    eq_filled_sse2(p, byte, n)
}

/// `AVX512F` + `AVX512BW` and OS XSAVE state for ZMM / opmask.
#[cfg(all(target_arch = "x86_64", not(kani)))]
fn has_avx512bw() -> bool {
    #[cfg(all(target_feature = "avx512f", target_feature = "avx512bw"))]
    {
        true
    }
    #[cfg(not(all(target_feature = "avx512f", target_feature = "avx512bw")))]
    {
        use core::sync::atomic::{AtomicU8, Ordering};
        static CACHED: AtomicU8 = AtomicU8::new(0);
        match CACHED.load(Ordering::Relaxed) {
            2 => true,
            1 => false,
            _ => {
                let yes = unsafe { detect_avx512bw() };
                CACHED.store(if yes { 2 } else { 1 }, Ordering::Relaxed);
                yes
            }
        }
    }
}

#[cfg(all(
    target_arch = "x86_64",
    not(kani),
    not(all(target_feature = "avx512f", target_feature = "avx512bw"))
))]
unsafe fn detect_avx512bw() -> bool {
    use core::arch::x86_64::{__cpuid, __cpuid_count, _xgetbv};
    let max = __cpuid(0).eax;
    if max < 7 {
        return false;
    }
    let e1 = __cpuid(1);
    // ECX.OSXSAVE
    if e1.ecx & (1 << 27) == 0 {
        return false;
    }
    let xcr0 = _xgetbv(0);
    // XMM | YMM | opmask | ZMM_Hi256 | Hi16_ZMM
    const XCR0_AVX512: u64 = (1 << 1) | (1 << 2) | (1 << 5) | (1 << 6) | (1 << 7);
    if xcr0 & XCR0_AVX512 != XCR0_AVX512 {
        return false;
    }
    let ebx = __cpuid_count(7, 0).ebx;
    const AVX512F: u32 = 1 << 16;
    const AVX512BW: u32 = 1 << 30;
    ebx & (AVX512F | AVX512BW) == AVX512F | AVX512BW
}

#[cfg(all(target_arch = "x86_64", not(kani)))]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn fill_avx512(p: *mut u8, byte: u8, n: usize) -> usize {
    use core::arch::x86_64::{__m512i, _mm512_set1_epi8, _mm512_storeu_si512};
    let v = _mm512_set1_epi8(byte as i8);
    let mut i = 0usize;
    while i + 256 <= n {
        _mm512_storeu_si512(p.add(i).cast::<__m512i>(), v);
        _mm512_storeu_si512(p.add(i + 64).cast::<__m512i>(), v);
        _mm512_storeu_si512(p.add(i + 128).cast::<__m512i>(), v);
        _mm512_storeu_si512(p.add(i + 192).cast::<__m512i>(), v);
        i += 256;
    }
    while i + 64 <= n {
        _mm512_storeu_si512(p.add(i).cast::<__m512i>(), v);
        i += 64;
    }
    i
}

#[cfg(all(target_arch = "x86_64", not(kani)))]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn copy_avx512(dst: *mut u8, src: *const u8, n: usize) -> usize {
    use core::arch::x86_64::{__m512i, _mm512_loadu_si512, _mm512_storeu_si512};
    let mut i = 0usize;
    while i + 256 <= n {
        let a = _mm512_loadu_si512(src.add(i).cast::<__m512i>());
        let b = _mm512_loadu_si512(src.add(i + 64).cast::<__m512i>());
        let c = _mm512_loadu_si512(src.add(i + 128).cast::<__m512i>());
        let d = _mm512_loadu_si512(src.add(i + 192).cast::<__m512i>());
        _mm512_storeu_si512(dst.add(i).cast::<__m512i>(), a);
        _mm512_storeu_si512(dst.add(i + 64).cast::<__m512i>(), b);
        _mm512_storeu_si512(dst.add(i + 128).cast::<__m512i>(), c);
        _mm512_storeu_si512(dst.add(i + 192).cast::<__m512i>(), d);
        i += 256;
    }
    while i + 64 <= n {
        let v = _mm512_loadu_si512(src.add(i).cast::<__m512i>());
        _mm512_storeu_si512(dst.add(i).cast::<__m512i>(), v);
        i += 64;
    }
    i
}

#[cfg(all(target_arch = "x86_64", not(kani)))]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn eq_filled_avx512(p: *const u8, byte: u8, n: usize) -> Result<usize, ()> {
    use core::arch::x86_64::{
        __m512i, _mm512_cmpeq_epi8_mask, _mm512_loadu_si512, _mm512_set1_epi8,
    };
    let v = _mm512_set1_epi8(byte as i8);
    let mut i = 0usize;
    while i + 64 <= n {
        let loaded = _mm512_loadu_si512(p.add(i).cast::<__m512i>());
        if _mm512_cmpeq_epi8_mask(loaded, v) != u64::MAX {
            return Err(());
        }
        i += 64;
    }
    Ok(i)
}

#[cfg(all(target_arch = "x86_64", not(kani)))]
unsafe fn fill_sse2(p: *mut u8, byte: u8, n: usize) {
    use core::arch::x86_64::{__m128i, _mm_set1_epi8, _mm_storeu_si128};
    let v = _mm_set1_epi8(byte as i8);
    let mut i = 0usize;
    while i + 16 <= n {
        _mm_storeu_si128(p.add(i).cast::<__m128i>(), v);
        i += 16;
    }
    while i < n {
        *p.add(i) = byte;
        i += 1;
    }
}

#[cfg(all(target_arch = "x86_64", not(kani)))]
unsafe fn eq_filled_sse2(p: *const u8, byte: u8, n: usize) -> bool {
    use core::arch::x86_64::{
        __m128i, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi8,
    };
    let v = _mm_set1_epi8(byte as i8);
    let mut i = 0usize;
    while i + 16 <= n {
        let loaded = _mm_loadu_si128(p.add(i).cast::<__m128i>());
        let eq = _mm_cmpeq_epi8(loaded, v);
        if _mm_movemask_epi8(eq) != 0xFFFF {
            return false;
        }
        i += 16;
    }
    while i < n {
        if *p.add(i) != byte {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(all(target_arch = "aarch64", not(kani)))]
unsafe fn fill_neon(p: *mut u8, byte: u8, n: usize) {
    use core::arch::aarch64::{vdupq_n_u8, vst1q_u8};
    let v = vdupq_n_u8(byte);
    let mut i = 0usize;
    while i + 16 <= n {
        vst1q_u8(p.add(i), v);
        i += 16;
    }
    while i < n {
        *p.add(i) = byte;
        i += 1;
    }
}

#[cfg(all(target_arch = "aarch64", not(kani)))]
unsafe fn eq_filled_neon(p: *const u8, byte: u8, n: usize) -> bool {
    use core::arch::aarch64::{vceqq_u8, vdupq_n_u8, vld1q_u8, vminvq_u8};
    let v = vdupq_n_u8(byte);
    let mut i = 0usize;
    while i + 16 <= n {
        let loaded = vld1q_u8(p.add(i));
        let eq = vceqq_u8(loaded, v);
        if vminvq_u8(eq) != 0xFF {
            return false;
        }
        i += 16;
    }
    while i < n {
        if *p.add(i) != byte {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_and_eq() {
        let mut b = [0u8; 40];
        unsafe {
            fill(b.as_mut_ptr(), 0xDE, 40);
            assert!(eq_filled(b.as_ptr(), 0xDE, 40));
            b[17] = 0;
            assert!(!eq_filled(b.as_ptr(), 0xDE, 40));
        }
    }

    #[test]
    fn fill_and_eq_large() {
        let mut b = [0u8; 1024];
        unsafe {
            fill(b.as_mut_ptr(), 0xAB, 1024);
            assert!(eq_filled(b.as_ptr(), 0xAB, 1024));
            b[0] = 0;
            assert!(!eq_filled(b.as_ptr(), 0xAB, 1024));
            fill(b.as_mut_ptr(), 0xAB, 1024);
            b[1023] = 0;
            assert!(!eq_filled(b.as_ptr(), 0xAB, 1024));
            fill(b.as_mut_ptr(), 0xCD, 257);
            assert!(eq_filled(b.as_ptr(), 0xCD, 257));
            assert_eq!(b[257], 0xAB);
        }
    }

    #[test]
    fn copy_overlap_free() {
        let src = [1u8, 2, 3, 4, 5];
        let mut dst = [0u8; 5];
        unsafe {
            copy(dst.as_mut_ptr(), src.as_ptr(), 5);
        }
        assert_eq!(dst, src);
    }

    #[test]
    fn copy_large() {
        let mut src = [0u8; 1000];
        let mut dst = [0u8; 1000];
        for (i, b) in src.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        unsafe {
            copy(dst.as_mut_ptr(), src.as_ptr(), 1000);
        }
        assert_eq!(dst, src);
    }
}
