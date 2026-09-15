//! mmap / mprotect model proofs (`cfg(kani)` OS backend).

#[cfg(test)]
mod tests {
    #[test]
    fn purge_null_is_noop() {
        unsafe {
            crate::os::purge(core::ptr::null_mut(), 0);
            crate::os::purge(core::ptr::null_mut(), 64);
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use crate::os::{self, Mapping, PROT_NONE, PROT_READ, PROT_WRITE};

    #[kani::proof]
    fn mmap_anon_aligned_and_size() {
        unsafe {
            let p = os::mmap_anon(0);
            assert!(p.is_null());
            let q = os::mmap_anon(64);
            assert!(!q.is_null());
            assert_eq!(q as usize % os::page_size(), 0);
            assert!(os::region_prot(q as *const u8).is_some());
            os::munmap(q, 64);
            assert!(os::region_prot(q as *const u8).is_none());
        }
    }

    #[kani::proof]
    fn mmap_aligned_honors_align() {
        unsafe {
            let p = os::mmap_aligned(4096, 4096);
            assert!(!p.is_null());
            assert_eq!(p as usize % 4096, 0);
            os::munmap(p, 4096);
        }
    }

    #[kani::proof]
    fn protect_none_then_unprotect() {
        unsafe {
            let p = os::mmap_anon(8192);
            assert!(!p.is_null());
            let rw = PROT_READ | PROT_WRITE;
            assert_eq!(os::region_prot(p as *const u8), Some(rw));
            assert!(os::protect(p, 4096));
            assert_eq!(os::region_prot(p as *const u8), Some(PROT_NONE));
            assert_eq!(os::region_prot(p.add(4096) as *const u8), Some(rw));
            assert!(os::unprotect(p, 4096));
            assert_eq!(os::region_prot(p as *const u8), Some(rw));
            os::munmap(p, 8192);
        }
    }

    #[kani::proof]
    fn mapping_leak_stays_mapped() {
        unsafe {
            let m = Mapping::anon(4096).unwrap();
            let q = m.leak();
            assert!(os::region_prot(q as *const u8).is_some());
            os::munmap(q, 4096);
        }
    }

    #[kani::proof]
    fn munmap_then_remap_reuses() {
        unsafe {
            let p = os::mmap_anon(4096);
            os::munmap(p, 4096);
            let q = os::mmap_anon(4096);
            assert!(!q.is_null());
            os::munmap(q, 4096);
        }
    }
}
