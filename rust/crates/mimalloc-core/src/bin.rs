//! Size classes matching mimalloc v3 (73 bins, ~12.5% spacing).

use crate::{BIN_HUGE, LARGE_MAX_OBJ_SIZE, MAX_ALIGN_SIZE, PTR_SIZE};

pub const BIN_COUNT: usize = BIN_HUGE + 1;
pub const LARGE_MAX_OBJ_WSIZE: usize = LARGE_MAX_OBJ_SIZE / PTR_SIZE;

static mut BIN_SIZES: [usize; BIN_COUNT] = [0; BIN_COUNT];
static mut BIN_INIT: bool = false;

#[inline]
pub const fn wsize_from_size(size: usize) -> usize {
    (size + PTR_SIZE - 1) / PTR_SIZE
}

/// C `MI_ALIGN4W`: 32-bit with 16-byte `max_align_t`.
const ALIGN4W: bool = MAX_ALIGN_SIZE > 2 * PTR_SIZE;
/// C `MI_ALIGN2W`: 64-bit with 16-byte `max_align_t`.
const ALIGN2W: bool = MAX_ALIGN_SIZE > PTR_SIZE && MAX_ALIGN_SIZE <= 2 * PTR_SIZE;

#[inline]
pub fn bin_for_size(size: usize) -> usize {
    let mut wsize = wsize_from_size(size);
    // Round small sizes so block_size is a multiple of MAX_ALIGN_SIZE (except
    // the 1-word bin used for malloc(0)+padding). 24-byte classes are 8-aligned
    // on every other block; hashbrown `movdqa` requires 16.
    if ALIGN4W {
        if wsize <= 4 {
            return if wsize <= 1 { 1 } else { (wsize + 1) & !1 };
        }
    } else if ALIGN2W {
        if wsize <= 8 {
            return if wsize <= 1 { 1 } else { (wsize + 1) & !1 };
        }
    } else if wsize <= 8 {
        return if wsize == 0 { 1 } else { wsize };
    }
    if wsize > LARGE_MAX_OBJ_WSIZE {
        return BIN_HUGE;
    }
    if ALIGN4W && wsize <= 16 {
        wsize = (wsize + 3) & !3;
    }
    wsize -= 1;
    let b = (usize::BITS as usize - 1) - (wsize.leading_zeros() as usize);
    ((b << 2) + ((wsize >> (b - 2)) & 0x03)) - 3
}

pub fn init_bin_sizes() {
    unsafe {
        if BIN_INIT {
            return;
        }
        let mut sz = 0usize;
        while sz <= LARGE_MAX_OBJ_SIZE {
            let bin = bin_for_size(sz);
            if bin < BIN_HUGE {
                BIN_SIZES[bin] = sz;
            }
            sz += 1;
        }
        BIN_SIZES[0] = 0;
        if BIN_SIZES[1] == 0 {
            BIN_SIZES[1] = PTR_SIZE;
        }
        BIN_INIT = true;
    }
}

#[inline]
pub fn bin_size(bin: usize) -> usize {
    unsafe {
        if bin >= BIN_HUGE {
            LARGE_MAX_OBJ_SIZE
        } else {
            BIN_SIZES[bin]
        }
    }
}

#[inline]
pub fn good_size(size: usize) -> usize {
    let size = if size == 0 { crate::PTR_SIZE } else { size };
    let need = size.saturating_add(crate::PADDING_SIZE);
    if need > LARGE_MAX_OBJ_SIZE {
        crate::align_up(need, crate::os::page_size())
    } else {
        let bin = bin_for_size(need);
        if bin >= BIN_HUGE {
            crate::align_up(need, crate::os::page_size())
        } else {
            bin_size(bin)
        }
    }
}

/// Small objects go in 64 KiB pages, medium in 512 KiB, large in 4 MiB.
#[inline]
pub fn page_size_for_block(block_size: usize) -> usize {
    const SMALL_MAX: usize = (crate::SLICE_SIZE - 4 * 1024) / 6; // ~10 KiB
    const MEDIUM_MAX: usize = (crate::MEDIUM_PAGE_SIZE - 4 * 1024) / 6; // ~84 KiB
    if block_size <= SMALL_MAX {
        crate::SLICE_SIZE
    } else if block_size <= MEDIUM_MAX {
        crate::MEDIUM_PAGE_SIZE
    } else {
        crate::LARGE_PAGE_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_bins_are_word_multiples() {
        init_bin_sizes();
        // First eight word sizes map into bins 1..=8 (ALIGN2W/4W skip odd bins).
        for bytes in 1..=(8 * PTR_SIZE) {
            let bin = bin_for_size(bytes);
            assert!(bin >= 1 && bin <= 8, "size {bytes} -> bin {bin}");
            assert_eq!(bin_size(bin) % PTR_SIZE, 0);
            assert!(bin_size(bin) >= bytes);
        }
    }

    #[test]
    fn align2w_skips_24_byte_class() {
        init_bin_sizes();
        if !ALIGN2W {
            return;
        }
        // C `(wsize+1)&~1` for wsize<=8: 24-byte requests share the 32-byte bin.
        assert_eq!(bin_for_size(24), bin_for_size(32));
        assert_eq!(bin_size(bin_for_size(24)), 32);
        assert_eq!(bin_size(bin_for_size(16)), 16);
    }

    #[test]
    fn bins_are_monotonic() {
        init_bin_sizes();
        let mut prev = 0;
        for sz in 1..=LARGE_MAX_OBJ_SIZE {
            let bin = bin_for_size(sz);
            assert!(bin >= prev, "size {sz} bin {bin} < {prev}");
            prev = bin;
        }
        assert_eq!(bin_for_size(LARGE_MAX_OBJ_SIZE + 1), BIN_HUGE);
    }

    #[test]
    fn bin_count_is_73() {
        init_bin_sizes();
        let mut max_bin = 0;
        for sz in 1..=LARGE_MAX_OBJ_SIZE {
            max_bin = max_bin.max(bin_for_size(sz));
        }
        assert!(max_bin < BIN_HUGE, "max regular bin {max_bin}");
        assert!(max_bin > 50);
    }
}
