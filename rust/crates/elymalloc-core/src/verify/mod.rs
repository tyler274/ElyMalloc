//! Kani proofs and matching host tests.
//!
//! Layout math, the mmap/TLS models, SIMD lane twins, and real `Page`
//! mutation. Production backends remain unix / `core::arch`. Install Kani
//! via `nix develop` or `cargo kani setup`, then `cargo kani -p elymalloc-core`.

mod layout;
mod mem;
mod os;
mod page;
mod tls;

use crate::quarantine::{Insert, Ring};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarantine_ring_insert_evict_dup_fixed() {
        let mut r = Ring::<4>::new();
        match r.insert(1, 4, 16) {
            Insert::Held { n, .. } => assert_eq!(n, 0),
            _ => panic!("first insert must hold"),
        }
        match r.insert(1, 4, 16) {
            Insert::Duplicate => {}
            _ => panic!("duplicate"),
        }
        let _ = r.insert(2, 4, 16);
        assert!(r.contains(1) || r.contains(2));
        assert!(!r.contains(0));
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(12)]
    fn quarantine_ring_insert_evict_dup() {
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
