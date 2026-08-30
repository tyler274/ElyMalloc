//! libc-less `#[global_allocator]` smoke used as a wasm32 binary and a host test.
#![no_std]

extern crate alloc;

use mimalloc_core::Mimalloc;

#[global_allocator]
static ALLOC: Mimalloc = Mimalloc;

/// Exercise Vec/Box grow, shrink, and contents through `Mimalloc`.
/// Returns 0 on success, a non-zero probe id on failure.
pub fn run() -> i32 {
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    let mut v = Vec::new();
    for i in 0..2048u32 {
        v.push(i);
    }
    if v.len() != 2048 || v[0] != 0 || v[2047] != 2047 {
        return 1;
    }
    v.reserve(32 * 1024);
    if v[100] != 100 {
        return 2;
    }

    let b = Box::new([0xABu8; 256]);
    if b[0] != 0xAB || b[255] != 0xAB {
        return 3;
    }
    drop(b);

    drop(v);
    let mut w: Vec<u8> = Vec::with_capacity(16);
    w.resize(128 * 1024, 0xCD);
    if w[0] != 0xCD || w[w.len() - 1] != 0xCD {
        return 4;
    }
    w.truncate(32);
    w.shrink_to_fit();
    if w.len() != 32 || w[0] != 0xCD {
        return 5;
    }

    // Direct GlobalAlloc path (aligned realloc).
    {
        use core::alloc::{GlobalAlloc, Layout};
        let layout = match Layout::from_size_align(48, 32) {
            Ok(l) => l,
            Err(_) => return 6,
        };
        unsafe {
            let p = ALLOC.alloc(layout);
            if p.is_null() || (p as usize) % 32 != 0 {
                return 7;
            }
            core::ptr::write_bytes(p, 0xEF, 48);
            if *p != 0xEF {
                ALLOC.dealloc(p, layout);
                return 8;
            }
            let q = ALLOC.realloc(p, layout, 96);
            if q.is_null() || (q as usize) % 32 != 0 || *q != 0xEF {
                if !q.is_null() {
                    ALLOC.dealloc(q, Layout::from_size_align(96, 32).unwrap());
                }
                return 9;
            }
            ALLOC.dealloc(q, Layout::from_size_align(96, 32).unwrap());
        }
    }

    0
}
