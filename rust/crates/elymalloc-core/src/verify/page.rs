//! Ghost-page spec and real [`crate::page`] mutation.
//!
//! Kani walks `init_local_free` / pop / push / collect / `block_next` on a
//! stack `Page` (64 KiB `page::create` is out of CBMC's range). Host tests
//! cover `create` / padding / `destroy` on real mmap; OS proofs cover the
//! region table used by guards.

/// Synthetic accounting: `used + local_len == capacity`, `used >= thread_free_len`.
#[derive(Clone, Copy)]
struct GhostPage {
    capacity: u32,
    used: u32,
    local_len: u32,
    thread_free_len: u32,
}

impl GhostPage {
    fn new(capacity: u32) -> Self {
        Self {
            capacity,
            used: 0,
            local_len: capacity,
            thread_free_len: 0,
        }
    }

    fn inv(&self) -> bool {
        self.used >= self.thread_free_len
            && (self.used as u64) + (self.local_len as u64) == self.capacity as u64
    }

    fn pop(&mut self) -> bool {
        if self.local_len == 0 {
            return false;
        }
        self.local_len -= 1;
        self.used += 1;
        true
    }

    fn push(&mut self) -> bool {
        if self.used == 0 || self.used <= self.thread_free_len {
            return false;
        }
        self.used -= 1;
        self.local_len += 1;
        true
    }

    fn push_thread(&mut self) -> bool {
        if self.used <= self.thread_free_len {
            return false;
        }
        self.thread_free_len += 1;
        true
    }

    fn collect(&mut self) {
        self.used = self.used.saturating_sub(self.thread_free_len);
        self.local_len = self.local_len.saturating_add(self.thread_free_len);
        self.thread_free_len = 0;
    }
}

#[cfg(kani)]
unsafe fn list_len(pg: *mut crate::page::Page, mut p: *mut crate::page::Block, cap: u32) -> u32 {
    let mut n = 0u32;
    while !p.is_null() {
        n += 1;
        if n > cap {
            return n;
        }
        p = crate::page::block_next(pg, p);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghost_page_push_pop() {
        let mut p = GhostPage::new(8);
        assert!(p.inv());
        assert!(p.pop());
        assert!(p.pop());
        assert!(p.push());
        assert_eq!(p.used + p.local_len, p.capacity);
        assert!(p.inv());
        assert!(p.push_thread());
        assert!(p.inv());
        p.collect();
        assert_eq!(p.thread_free_len, 0);
        assert!(p.inv());
    }

    #[test]
    fn page_create_pop_padding_destroy_fixed() {
        crate::page_map::init();
        unsafe {
            let pg = crate::page::create(2048, crate::SLICE_SIZE, core::ptr::null_mut());
            assert!(!pg.is_null());
            assert_eq!((*pg).magic, crate::page::PAGE_MAGIC);
            let p = crate::page::pop_local(pg);
            assert!(!p.is_null());
            crate::page::write_padding(p, 8);
            assert_eq!(crate::page::usable_size(pg, p), 8);
            assert!(crate::page::check_free(pg, p));
            let base = (*pg).map_base;
            crate::page::destroy(pg);
            assert!(crate::page_map::get(base).is_null());
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::os;
    use crate::page::{self, Block, Page, PAGE_MAGIC};
    use core::ptr;
    use core::sync::atomic::AtomicPtr;

    unsafe fn stack_page(buf: &mut [u8], cap: usize, bs: usize) -> *mut Page {
        ptr::write_bytes(buf.as_mut_ptr(), 0, buf.len());
        let pg = buf.as_mut_ptr() as *mut Page;
        let off = core::mem::size_of::<Page>();
        let area = buf.as_mut_ptr().add(off);
        (*pg).magic = PAGE_MAGIC;
        (*pg).block_size = bs;
        (*pg).capacity = cap as u32;
        (*pg).used = 0;
        (*pg).thread_free = AtomicPtr::new(ptr::null_mut());
        (*pg).area = area;
        (*pg).map_base = buf.as_mut_ptr();
        (*pg).map_size = buf.len();
        (*pg).key1 = 1;
        (*pg).key2 = 2;
        page::init_local_free(pg, area, bs, cap);
        pg
    }

    #[kani::proof]
    #[kani::unwind(10)]
    fn ghost_page_capacity() {
        let cap: u32 = kani::any();
        kani::assume(cap >= 1 && cap <= 8);
        let mut p = GhostPage::new(cap);
        let steps: u8 = kani::any();
        kani::assume(steps <= 8);
        for _ in 0..steps {
            match kani::any() {
                0u8 => {
                    let _ = p.pop();
                }
                1 => {
                    let _ = p.push();
                }
                _ => {
                    let _ = p.push_thread();
                    if kani::any() {
                        p.collect();
                    }
                }
            }
            assert!(p.inv());
        }
    }

    #[kani::proof]
    #[kani::unwind(8)]
    fn page_linear_init_and_list() {
        unsafe {
            let mut buf = [0u8; 512];
            let pg = stack_page(&mut buf, 2, 32);
            assert_eq!((*pg).magic, PAGE_MAGIC);
            assert_eq!((*pg).used, 0);
            let n = list_len(pg, (*pg).local_free, 2);
            assert_eq!(n, 2);
        }
    }

    #[kani::proof]
    #[kani::unwind(12)]
    fn page_shuffle_pop_push() {
        unsafe {
            let mut buf = [0u8; 512];
            let pg = stack_page(&mut buf, 4, 32);
            let cap = (*pg).capacity;
            assert_eq!(cap, 4);
            let n = list_len(pg, (*pg).local_free, cap);
            assert_eq!(n, cap);
            let mut got = [ptr::null_mut::<u8>(); 4];
            let mut i = 0u32;
            while i < cap {
                let p = page::pop_local(pg);
                assert!(!p.is_null());
                assert!(page::contains(pg, p));
                assert!(page::is_block_start(pg, p));
                assert_eq!((*(p as *mut Block)).next, 0);
                got[i as usize] = p;
                i += 1;
            }
            assert!(page::pop_local(pg).is_null());
            assert_eq!((*pg).used, cap);
            i = 0;
            while i < cap {
                page::push_local(pg, got[i as usize]);
                i += 1;
            }
            assert_eq!((*pg).used, 0);
        }
    }

    #[kani::proof]
    #[kani::unwind(8)]
    fn page_thread_free_collect() {
        unsafe {
            let mut buf = [0u8; 512];
            let pg = stack_page(&mut buf, 2, 32);
            let p = page::pop_local(pg);
            assert!(!p.is_null());
            os::switch_thread(2);
            page::push_thread_free(pg, p);
            os::switch_thread(1);
            page::collect(pg);
            assert_eq!((*pg).used, 0);
            let n = list_len(pg, (*pg).local_free, (*pg).capacity);
            assert_eq!(n, (*pg).capacity);
        }
    }

    #[kani::proof]
    #[kani::should_panic]
    #[kani::unwind(8)]
    fn page_corrupt_next_panics() {
        unsafe {
            let mut buf = [0u8; 512];
            let pg = stack_page(&mut buf, 2, 32);
            let p = page::pop_local(pg);
            page::push_local(pg, p);
            let b = p as *mut Block;
            (*b).next = 0xDEAD_BEEF;
            let _ = page::block_next(pg, b);
        }
    }

    #[kani::proof]
    #[kani::unwind(8)]
    fn page_huge_one_pop() {
        unsafe {
            let mut buf = [0u8; 256];
            ptr::write_bytes(buf.as_mut_ptr(), 0, buf.len());
            let pg = buf.as_mut_ptr() as *mut Page;
            let area = buf.as_mut_ptr().add(core::mem::size_of::<Page>());
            (*pg).magic = PAGE_MAGIC;
            (*pg).block_size = 64;
            (*pg).capacity = 1;
            (*pg).local_free = area as *mut Block;
            (*pg).area = area;
            (*pg).map_base = buf.as_mut_ptr();
            (*pg).map_size = buf.len();
            (*pg).key1 = 1;
            (*pg).key2 = 2;
            let p = page::pop_local(pg);
            assert_eq!(p, area);
            assert!(page::pop_local(pg).is_null());
        }
    }
}
