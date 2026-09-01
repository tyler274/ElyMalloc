//! Integer model of [`crate::free_list::FreeList`] for Kani (no `BTreeMap`).
//!
//! Adjacent free ranges coalesce. First-fit never returns overlapping offsets.
//! Host tests mirror the proofs. Storage is a fixed-size array so CBMC does
//! not unwind `Vec` grow/fold (unwind 8 + `Vec` used tens of GB of RAM).

/// Sorted non-overlapping free ranges `(offset, length)`.
#[derive(Clone, Debug)]
pub struct VecFreeList {
    ranges: [(u64, u64); MAX_RANGES],
    n: usize,
    pub total: u64,
}

const MAX_RANGES: usize = 4;

impl VecFreeList {
    pub fn new(size: u64) -> Self {
        let mut ranges = [(0, 0); MAX_RANGES];
        let n = if size > 0 {
            ranges[0] = (0, size);
            1
        } else {
            0
        };
        Self {
            ranges,
            n,
            total: size,
        }
    }

    pub fn free_bytes(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0;
        while i < self.n {
            s += self.ranges[i].1;
            i += 1;
        }
        s
    }

    pub fn disjoint(&self) -> bool {
        if self.free_bytes() > self.total {
            return false;
        }
        let mut i = 0;
        while i + 1 < self.n {
            let (o0, l0) = self.ranges[i];
            let (o1, _) = self.ranges[i + 1];
            if o0 + l0 > o1 {
                return false;
            }
            i += 1;
        }
        true
    }

    fn align_up(x: u64, align: u64) -> u64 {
        let a = align.max(1);
        (x + (a - 1)) & !(a - 1)
    }

    fn remove(&mut self, i: usize) {
        let mut j = i;
        while j + 1 < self.n {
            self.ranges[j] = self.ranges[j + 1];
            j += 1;
        }
        self.n -= 1;
        self.ranges[self.n] = (0, 0);
    }

    fn insert(&mut self, i: usize, val: (u64, u64)) {
        let mut j = self.n;
        while j > i {
            self.ranges[j] = self.ranges[j - 1];
            j -= 1;
        }
        self.ranges[i] = val;
        self.n += 1;
    }

    pub fn alloc_first_fit(&mut self, size: u64, align: u64) -> Option<u64> {
        if size == 0 {
            return None;
        }
        let align = align.max(1);
        let mut i = 0usize;
        while i < self.n {
            let (off, len) = self.ranges[i];
            let aligned = Self::align_up(off, align);
            if aligned < off {
                i += 1;
                continue;
            }
            let pad = aligned - off;
            if pad + size > len {
                i += 1;
                continue;
            }
            let trail_off = aligned + size;
            let trail = (off + len).saturating_sub(trail_off);
            if pad == 0 {
                self.remove(i);
            } else {
                self.ranges[i] = (off, pad);
                i += 1;
            }
            if trail > 0 {
                self.insert(i, (trail_off, trail));
            }
            return Some(aligned);
        }
        None
    }

    pub fn free(&mut self, offset: u64, size: u64) {
        if size == 0 {
            return;
        }
        let mut start = offset;
        let mut len = size;
        let mut insert_at = 0usize;
        let mut i = 0usize;
        while i < self.n {
            let (off, l) = self.ranges[i];
            if off + l < start {
                insert_at = i + 1;
                i += 1;
                continue;
            }
            if off + l == start {
                start = off;
                len += l;
                self.remove(i);
                continue;
            }
            if start + len == off {
                len += l;
                self.remove(i);
                continue;
            }
            if off > start {
                insert_at = i;
                break;
            }
            insert_at = i + 1;
            i += 1;
        }
        self.insert(insert_at, (start, len));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_restores_total() {
        let mut f = VecFreeList::new(1000);
        let a = f.alloc_first_fit(100, 16).unwrap();
        let b = f.alloc_first_fit(200, 16).unwrap();
        assert!(b >= a + 100);
        f.free(a, 100);
        f.free(b, 200);
        assert_eq!(f.free_bytes(), 1000);
        assert_eq!(f.n, 1);
        assert!(f.disjoint());
    }

    #[test]
    fn first_fit_no_overlap() {
        let mut f = VecFreeList::new(64);
        let a = f.alloc_first_fit(8, 8).unwrap();
        let b = f.alloc_first_fit(8, 8).unwrap();
        assert!(a + 8 <= b || b + 8 <= a);
        assert!(f.disjoint());
        f.free(a, 8);
        assert_eq!(f.free_bytes() + 8, 64);
        f.free(b, 8);
        assert_eq!(f.free_bytes(), 64);
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(5)]
    fn alloc_free_restores_bytes() {
        let total: u64 = kani::any();
        kani::assume(total >= 16 && total <= 32);
        let mut f = VecFreeList::new(total);
        let size: u64 = kani::any();
        kani::assume(size >= 1 && size <= 8);
        if let Some(off) = f.alloc_first_fit(size, 1) {
            assert!(f.disjoint());
            assert!(off + size <= total);
            f.free(off, size);
            assert_eq!(f.free_bytes(), total);
            assert_eq!(f.n, 1);
        }
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn first_fit_never_overlaps() {
        let mut f = VecFreeList::new(32);
        let a = f.alloc_first_fit(4, 1);
        let b = f.alloc_first_fit(4, 1);
        assert!(f.disjoint());
        if let (Some(x), Some(y)) = (a, b) {
            assert!(x + 4 <= y || y + 4 <= x);
        }
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn coalesce_two_frees() {
        let mut f = VecFreeList::new(32);
        let a = f.alloc_first_fit(8, 1).unwrap();
        let b = f.alloc_first_fit(8, 1).unwrap();
        f.free(a, 8);
        f.free(b, 8);
        assert_eq!(f.free_bytes(), 32);
        assert!(f.disjoint());
    }
}
