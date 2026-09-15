//! Pointer-free layout, bin-adjacent, and address math.
//!
//! Unsafe page / page-map / arena code loads fields then calls these helpers
//! so Kani can prove the integer core without OS pointers. Invariants:
//!
//! - [`block_align`] is ≥ 16 and a power of two.
//! - [`meta_prefix`] places `Page` after a lead guard; the first block is
//!   aligned to `block_align`.
//! - [`in_range`] / [`is_block_start`] / [`block_start`] are the integer
//!   bodies of [`crate::page::contains`] / `is_block_start` / `block_start_of`.

use crate::align_up;
use crate::{MAX_ALLOC, PADDING_SIZE};

/// Largest power of two that divides `block_size`, at least 16
/// (`MI_PAGE_MIN_START_BLOCK_ALIGN`).
#[inline]
pub const fn block_align(block_size: usize) -> usize {
    if block_size == 0 {
        return 16;
    }
    let po2 = block_size & block_size.wrapping_neg();
    if po2 < 16 {
        16
    } else {
        po2
    }
}

/// `[lead guard][Page][mid guard][blocks…]` offsets.
///
/// `os_page` and `page_hdr` are passed in so the math does not call the OS.
/// Returns `(lead, meta, area_off)`.
#[inline]
pub fn meta_prefix(os_page: usize, page_hdr: usize, block_align: usize) -> (usize, usize, usize) {
    let os = os_page.max(1);
    let lead = os;
    let meta = align_up(page_hdr, os);
    let mid = os;
    let area0 = lead.saturating_add(meta).saturating_add(mid);
    (lead, meta, align_up(area0, block_align.max(1)))
}

/// How many equal `block_size` objects fit after `area_off` and before `end_guard`.
#[inline]
pub fn capacity(map_size: usize, area_off: usize, end_guard: usize, block_size: usize) -> usize {
    if block_size == 0 {
        return 0;
    }
    map_size
        .saturating_sub(area_off)
        .saturating_sub(end_guard)
        / block_size
}

/// `addr` is in `[start, start+len)` without wrapping `start+len`.
#[inline]
pub const fn in_range(start: usize, len: usize, addr: usize) -> bool {
    addr >= start && addr.wrapping_sub(start) < len
}

/// True if `addr` is in the block area and a multiple of `bs` from `area`.
#[inline]
pub fn is_block_start(area: usize, len: usize, bs: usize, addr: usize) -> bool {
    if bs == 0 || !in_range(area, len, addr) {
        return false;
    }
    addr.wrapping_sub(area) % bs == 0
}

/// Align `addr` down to a block start in `[area, area+len)`, or `None`.
#[inline]
pub fn block_start(area: usize, len: usize, bs: usize, addr: usize) -> Option<usize> {
    if bs == 0 || !in_range(area, len, addr) {
        return None;
    }
    let diff = addr.wrapping_sub(area);
    let adjust = if bs.is_power_of_two() {
        diff & (bs - 1)
    } else {
        diff % bs
    };
    Some(addr.wrapping_sub(adjust))
}

/// Two-level page-map indices (C `addr >> slice_shift`, then L2 / L1 split).
#[inline]
pub const fn page_map_split(addr: usize, slice_shift: usize, l2_bits: usize) -> (usize, usize) {
    let slice = addr >> slice_shift;
    let l2_mask = if l2_bits >= usize::BITS as usize {
        usize::MAX
    } else {
        (1usize << l2_bits).wrapping_sub(1)
    };
    let l2 = slice & l2_mask;
    let l1 = if l2_bits >= usize::BITS as usize {
        0
    } else {
        slice >> l2_bits
    };
    (l1, l2)
}

/// Production page-map L2 width ([`crate::page_map`] `L2_BITS` when not Kani).
pub const PAGE_MAP_L2_BITS: usize = 13;
/// Production L1 width (`L1_BITS` when not Kani).
pub const PAGE_MAP_L1_BITS: usize = 18;

/// Next bump position, or `None` if `align` is bad or the arena is exhausted.
#[inline]
pub fn arena_bump_next(
    pos: usize,
    size: usize,
    align: usize,
    arena_size: usize,
) -> Option<(usize, usize)> {
    if align == 0 || !align.is_power_of_two() {
        return None;
    }
    let aligned = align_up(pos, align);
    let new_pos = aligned.checked_add(size)?;
    if new_pos > arena_size {
        return None;
    }
    Some((aligned, new_pos))
}

/// Requests larger than this are `ENOMEM` (padding trailer must fit too).
#[inline]
pub const fn max_alloc_ok(size: usize) -> bool {
    size <= MAX_ALLOC.saturating_sub(PADDING_SIZE)
}

/// First aligned slot inside an over-mapped range, plus optional ASLR slot `k`.
///
/// Production [`crate::os`] jitter picks `k <= extra_slots`. The Kani mmap
/// model always uses `k = 0`. This lemma does not call the kernel.
#[cfg_attr(not(any(test, kani)), allow(dead_code))]
#[inline]
pub fn mmap_aligned_slot(
    raw: usize,
    total: usize,
    size: usize,
    align: usize,
    k: usize,
) -> Option<usize> {
    if size == 0 || align == 0 || !align.is_power_of_two() || total < size {
        return None;
    }
    let aligned = align_up(raw, align);
    if aligned < raw {
        return None;
    }
    let end = raw.saturating_add(total);
    let room = end.saturating_sub(aligned).saturating_sub(size);
    let extra_slots = room / align;
    if k > extra_slots {
        return None;
    }
    let chosen = aligned.saturating_add(k.saturating_mul(align));
    if chosen < aligned || chosen.saturating_add(size) > end {
        return None;
    }
    Some(chosen)
}
