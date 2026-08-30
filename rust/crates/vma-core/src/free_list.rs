//! Offset free-list used by virtual blocks and `VkDeviceMemory` suballocation.
//!
//! Adjacent free ranges coalesce. Strategies match VMA: first-fit (min time),
//! best-fit (min memory), min-offset.

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct FreeList {
    /// offset → size of a free range
    by_offset: BTreeMap<u64, u64>,
    pub total: u64,
    pub linear: bool,
    pub bump: u64,
    pub upper: u64,
}

impl FreeList {
    pub fn new(size: u64, linear: bool) -> Self {
        let mut by_offset = BTreeMap::new();
        if size > 0 && !linear {
            by_offset.insert(0, size);
        }
        Self {
            by_offset,
            total: size,
            linear,
            bump: 0,
            upper: size,
        }
    }

    pub fn allocated_bytes(&self) -> u64 {
        if self.linear {
            self.bump + (self.total - self.upper)
        } else {
            self.total - self.free_bytes()
        }
    }

    pub fn free_bytes(&self) -> u64 {
        self.by_offset.values().copied().sum()
    }

    pub fn unused_range_count(&self) -> u32 {
        self.by_offset.len() as u32
    }

    pub fn alloc(&mut self, size: u64, align: u64, flags: u32) -> Option<u64> {
        if size == 0 {
            return None;
        }
        let align = align.max(1);
        if self.linear {
            return self.alloc_linear(size, align, flags);
        }
        let strategy = flags & 0x70000;
        let min_offset = strategy == 0x40000;
        let best_fit = strategy == 0x10000 || strategy == 0;
        let mut first: Option<(u64, u64)> = None;
        let mut best: Option<(u64, u64, u64)> = None; // (waste, offset, aligned)
        for (&off, &len) in &self.by_offset {
            let aligned = align_up(off, align);
            if aligned < off {
                continue;
            }
            let pad = aligned - off;
            if pad + size > len {
                continue;
            }
            if !best_fit && !min_offset {
                first = Some((off, aligned));
                break;
            }
            let waste = len - (pad + size);
            let pick = if min_offset { aligned } else { waste };
            match best {
                None => best = Some((pick, off, aligned)),
                Some((w, _, _)) if min_offset && aligned < w => best = Some((aligned, off, aligned)),
                Some((w, _, _)) if best_fit && pick < w => best = Some((waste, off, aligned)),
                _ => {}
            }
        }
        if let Some((off, aligned)) = first {
            return Some(self.take(off, aligned, size));
        }
        best.map(|(_, off, aligned)| self.take(off, aligned, size))
    }

    fn alloc_linear(&mut self, size: u64, align: u64, flags: u32) -> Option<u64> {
        if flags & 0x40 != 0 {
            // upper stack
            let end = self.upper;
            let aligned_end = align_down(end, align);
            if aligned_end < size || aligned_end - size < self.bump {
                return None;
            }
            let start = aligned_end - size;
            if start < self.bump {
                return None;
            }
            self.upper = start;
            return Some(start);
        }
        let start = align_up(self.bump, align);
        let end = start.checked_add(size)?;
        if end > self.upper {
            return None;
        }
        self.bump = end;
        Some(start)
    }

    fn take(&mut self, free_off: u64, aligned: u64, size: u64) -> u64 {
        let len = self.by_offset.remove(&free_off).unwrap();
        let lead = aligned - free_off;
        let trail_off = aligned + size;
        let trail = (free_off + len).saturating_sub(trail_off);
        if lead > 0 {
            self.by_offset.insert(free_off, lead);
        }
        if trail > 0 {
            self.by_offset.insert(trail_off, trail);
        }
        aligned
    }

    pub fn free(&mut self, offset: u64, size: u64) {
        if self.linear || size == 0 {
            return;
        }
        let mut start = offset;
        let mut len = size;
        let prev = self.by_offset.range(..offset).next_back().map(|(&o, &l)| (o, l));
        if let Some((poff, plen)) = prev {
            if poff + plen == offset {
                self.by_offset.remove(&poff);
                start = poff;
                len += plen;
            }
        }
        let next = self.by_offset.range(offset + 1..).next().map(|(&o, &l)| (o, l));
        if let Some((noff, nlen)) = next {
            if start + len == noff {
                self.by_offset.remove(&noff);
                len += nlen;
            }
        }
        // also if we abut exactly offset key
        if let Some(nlen) = self.by_offset.remove(&(offset + size)) {
            len += nlen;
        }
        self.by_offset.insert(start, len);
    }

    pub fn clear(&mut self) {
        self.by_offset.clear();
        if !self.linear && self.total > 0 {
            self.by_offset.insert(0, self.total);
        }
        self.bump = 0;
        self.upper = self.total;
    }

    pub fn is_empty_of_allocs(&self) -> bool {
        if self.linear {
            self.bump == 0 && self.upper == self.total
        } else {
            self.free_bytes() == self.total
        }
    }
}

pub fn align_up(x: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two() || align == 1);
    let a = align.max(1);
    (x + a - 1) & !(a - 1)
}

pub fn align_down(x: u64, align: u64) -> u64 {
    let a = align.max(1);
    x & !(a - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_and_reuse() {
        let mut f = FreeList::new(1000, false);
        let a = f.alloc(100, 16, 0x20000).unwrap();
        let b = f.alloc(200, 16, 0x20000).unwrap();
        assert!(b >= a + 100);
        f.free(a, 100);
        let c = f.alloc(100, 16, 0x20000).unwrap();
        assert_eq!(c, a);
        f.free(b, 200);
        f.free(c, 100);
        assert_eq!(f.free_bytes(), 1000);
    }

    #[test]
    fn linear_bump() {
        let mut f = FreeList::new(1000, true);
        let a = f.alloc(100, 8, 0).unwrap();
        let b = f.alloc(100, 8, 0).unwrap();
        assert!(b > a);
        assert!(f.alloc(1000, 1, 0).is_none());
    }
}
