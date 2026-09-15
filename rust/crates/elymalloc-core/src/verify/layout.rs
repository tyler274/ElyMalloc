//! Layout / bin / padding integer proofs.

use crate::layout;
use crate::page::{encode_canary, padded_need, request_size, CANARY_FREED};
use crate::{align_up, bin, BIN_HUGE};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip_fixed() {
        use crate::page::{decode_addr, encode_addr};
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
            let b = bin::bin_for_size(size);
            assert!(b >= 1 && b <= BIN_HUGE, "size {size} -> bin {b}");
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
    fn block_align_and_meta_prefix_fixed() {
        assert_eq!(layout::block_align(0), 16);
        assert_eq!(layout::block_align(32), 32);
        assert_eq!(layout::block_align(48), 16);
        let (lead, meta, area) = layout::meta_prefix(4096, 192, 16);
        assert_eq!(lead, 4096);
        assert!(area >= lead + meta + 4096);
        assert_eq!(area % 16, 0);
        let cap = layout::capacity(65536, area, 4096, 32);
        assert!(area + cap * 32 + 4096 <= 65536);
    }

    #[test]
    fn in_range_and_block_start_fixed() {
        assert!(layout::in_range(100, 50, 100));
        assert!(layout::in_range(100, 50, 149));
        assert!(!layout::in_range(100, 50, 150));
        assert!(layout::is_block_start(100, 64, 16, 116));
        assert!(!layout::is_block_start(100, 64, 16, 108));
        assert_eq!(layout::block_start(100, 64, 16, 108), Some(100));
    }

    #[test]
    fn page_map_split_fixed() {
        let (l1, l2) = layout::page_map_split(0x1234_0000, 16, 13);
        assert!(l2 < (1 << 13));
        let _ = l1;
    }

    #[test]
    fn arena_bump_and_max_alloc_fixed() {
        let (a, n) = layout::arena_bump_next(8, 16, 16, 64).unwrap();
        assert_eq!(a, 16);
        assert_eq!(n, 32);
        assert!(layout::arena_bump_next(0, 16, 3, 64).is_none());
        assert!(layout::max_alloc_ok(16));
        assert!(!layout::max_alloc_ok(usize::MAX));
    }

    #[test]
    fn mmap_aligned_slot_k0_fits() {
        let raw = 0x1000;
        let size = 0x1000;
        let align = 0x1000;
        let total = size + align + 0x1000;
        let p = layout::mmap_aligned_slot(raw, total, size, align, 0).unwrap();
        assert_eq!(p % align, 0);
        assert!(p >= raw && p + size <= raw + total);
        let extra = ((raw + total) - p - size) / align;
        if extra > 0 {
            let q = layout::mmap_aligned_slot(raw, total, size, align, extra).unwrap();
            assert_eq!(q % align, 0);
            assert!(q + size <= raw + total);
        }
        assert!(layout::mmap_aligned_slot(raw, total, size, align, extra + 1).is_none());
    }

    #[test]
    fn padding_delta_usable() {
        let block = 32usize;
        let user = 8usize;
        let delta = block - crate::PADDING_SIZE - user;
        assert!(delta < block);
        let usable = block - crate::PADDING_SIZE - delta;
        assert_eq!(usable, user);
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::page::{decode_addr, encode_addr};

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
    fn block_align_props() {
        let bs: usize = kani::any();
        kani::assume(bs <= 4096);
        let a = layout::block_align(bs);
        assert!(a >= 16);
        assert!(a.is_power_of_two());
        if bs != 0 {
            let po2 = bs & bs.wrapping_neg();
            assert_eq!(a, po2.max(16));
        }
    }

    #[kani::proof]
    fn meta_prefix_and_capacity() {
        let os: usize = if kani::any() { 4096 } else { 16384 };
        let hdr: usize = kani::any();
        kani::assume(hdr >= 64 && hdr <= 512);
        let ba: usize = layout::block_align(32);
        let (lead, meta, area) = layout::meta_prefix(os, hdr, ba);
        assert_eq!(lead, os);
        assert!(area >= lead.saturating_add(meta).saturating_add(os));
        assert_eq!(area % ba.max(1), 0);
        let map: usize = 65536;
        let cap = layout::capacity(map, area, os, 32);
        assert!(area.saturating_add(cap.saturating_mul(32)).saturating_add(os) <= map);
    }

    #[kani::proof]
    fn in_range_block_start() {
        let area: usize = 4096;
        let bs: usize = 32;
        let cap: usize = kani::any();
        kani::assume(cap >= 1 && cap <= 8);
        let len = cap * bs;
        let addr: usize = kani::any();
        kani::assume(addr >= area && addr < area + len);
        let in_r = layout::in_range(area, len, addr);
        assert!(in_r);
        let st = layout::is_block_start(area, len, bs, addr);
        assert_eq!(st, (addr - area) % bs == 0);
        let b = layout::block_start(area, len, bs, addr).unwrap();
        assert!(layout::in_range(area, len, b));
        assert_eq!((b - area) % bs, 0);
    }

    #[kani::proof]
    fn page_map_split_prod() {
        let addr: usize = kani::any();
        kani::assume(addr < (1usize << 47));
        let (l1, l2) = layout::page_map_split(addr, 16, layout::PAGE_MAP_L2_BITS);
        assert!(l2 < (1 << layout::PAGE_MAP_L2_BITS));
        assert!(l1 < (1 << layout::PAGE_MAP_L1_BITS));
    }

    #[kani::proof]
    fn arena_bump_next_ok() {
        let pos: usize = kani::any();
        let size: usize = kani::any();
        let arena: usize = kani::any();
        kani::assume(pos <= 256 && size >= 1 && size <= 64 && arena <= 512);
        let align: usize = 16;
        if let Some((a, n)) = layout::arena_bump_next(pos, size, align, arena) {
            assert_eq!(a % align, 0);
            assert!(n <= arena);
            assert!(n == a + size);
        }
    }

    #[kani::proof]
    fn max_alloc_ok_no_overflow() {
        let size: usize = kani::any();
        let ok = layout::max_alloc_ok(size);
        assert_eq!(ok, size <= crate::MAX_ALLOC.saturating_sub(crate::PADDING_SIZE));
    }

    #[kani::proof]
    fn decode_then_range() {
        let k1: usize = kani::any();
        let k2: usize = kani::any();
        let area: usize = 4096;
        let len: usize = 256;
        let addr: usize = kani::any();
        kani::assume(addr >= area && addr < area + len);
        let enc = encode_addr(k1, k2, addr);
        let d = decode_addr(k1, k2, enc);
        assert_eq!(d, addr);
        assert_eq!(
            layout::is_block_start(area, len, 32, d),
            (d - area) % 32 == 0
        );
    }

    #[kani::proof]
    fn padding_delta() {
        let block: usize = kani::any();
        kani::assume(block > crate::PADDING_SIZE && block <= 256);
        let user: usize = kani::any();
        kani::assume(user <= block - crate::PADDING_SIZE);
        let delta = block - crate::PADDING_SIZE - user;
        assert!(delta < block);
        assert_eq!(block - crate::PADDING_SIZE - delta, user);
    }

    #[kani::proof]
    fn mmap_jitter_slot_fits() {
        let raw: usize = 4096;
        let size: usize = 4096;
        let align: usize = 4096;
        let extra: usize = kani::any();
        kani::assume(extra <= 4);
        let total = size + align + extra * align;
        let k: usize = kani::any();
        kani::assume(k <= extra);
        let p = layout::mmap_aligned_slot(raw, total, size, align, k).unwrap();
        assert_eq!(p % align, 0);
        assert!(p + size <= raw + total);
    }

    #[kani::proof]
    fn page_size_for_block_classes() {
        let bs: usize = kani::any();
        kani::assume(bs > 0 && bs <= 1024);
        let psz = bin::page_size_for_block(bs);
        assert!(psz == crate::SLICE_SIZE || psz == crate::MEDIUM_PAGE_SIZE || psz == crate::LARGE_PAGE_SIZE);
    }
}
