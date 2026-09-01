//! libc-less `#[global_allocator]` smoke: host unit test and wasm32 binary.
//!
//! The wasm module must not import libc `malloc`. Probe ids in [`run`] are
//! stable so harness logs stay comparable.
//!
//! WASM is sequential only (single `thread_id`). `memory.grow` cannot shrink;
//! `munmap` / guard pages / threads are out of scope.

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

/// Grow/realloc/churn/OOM stress. Sequential only (wasm has one thread).
/// Returns 0 on success, a probe id ≥ 10 on failure.
pub fn run_stress() -> i32 {
    use alloc::vec::Vec;
    use core::alloc::{GlobalAlloc, Layout};

    let sizes: [usize; 6] = [0, 1, 16, 4096, 65536, 1024 * 1024];
    for (i, &sz) in sizes.iter().enumerate() {
        let layout = match Layout::from_size_align(sz.max(1), 16) {
            Ok(l) => l,
            Err(_) => return 10 + i as i32,
        };
        unsafe {
            let p = ALLOC.alloc(layout);
            if sz != 0 && p.is_null() {
                return 20 + i as i32;
            }
            if !p.is_null() {
                if sz != 0 {
                    core::ptr::write_bytes(p, 0x11, sz.min(64).max(1).min(sz));
                }
                let new_sz = if sz == 0 { 32 } else { sz / 2 + 1 };
                let q = ALLOC.realloc(p, layout, new_sz);
                if q.is_null() {
                    ALLOC.dealloc(p, layout);
                    return 30 + i as i32;
                }
                let new_layout = Layout::from_size_align(new_sz, 16).unwrap();
                ALLOC.dealloc(q, new_layout);
            }
        }
    }

    unsafe {
        let zlayout = Layout::from_size_align(64, 8).unwrap();
        let z = ALLOC.alloc_zeroed(zlayout);
        if z.is_null() {
            return 40;
        }
        if *z != 0 || *z.add(63) != 0 {
            ALLOC.dealloc(z, zlayout);
            return 41;
        }
        ALLOC.dealloc(z, zlayout);

        let aligned = Layout::from_size_align(48, 64).unwrap();
        let p = ALLOC.alloc(aligned);
        if p.is_null() || (p as usize) % 64 != 0 {
            if !p.is_null() {
                ALLOC.dealloc(p, aligned);
            }
            return 42;
        }
        ALLOC.dealloc(p, aligned);

        ALLOC.dealloc(
            core::ptr::null_mut(),
            Layout::from_size_align(8, 8).unwrap(),
        );
        let mut stack = [0u8; 16];
        mimalloc_core::alloc::free(stack.as_mut_ptr());
    }

    let mut blocks: Vec<*mut u8> = Vec::new();
    const N: usize = 64;
    let layout = Layout::from_size_align(128, 16).unwrap();
    for _ in 0..N {
        unsafe {
            let p = ALLOC.alloc(layout);
            if p.is_null() {
                return 50;
            }
            core::ptr::write(p, 0x22u8);
            blocks.push(p);
        }
    }
    for i in (0..N).step_by(2) {
        unsafe {
            ALLOC.dealloc(blocks[i], layout);
            blocks[i] = core::ptr::null_mut();
        }
    }
    for i in (1..N).step_by(2) {
        unsafe {
            let q = ALLOC.realloc(blocks[i], layout, 256);
            if q.is_null() {
                return 51;
            }
            ALLOC.dealloc(q, Layout::from_size_align(256, 16).unwrap());
            blocks[i] = core::ptr::null_mut();
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        let huge = match Layout::from_size_align(1024 * 1024 * 1024, 16) {
            Ok(l) => l,
            Err(_) => return 60,
        };
        unsafe {
            let p = ALLOC.alloc(huge);
            if !p.is_null() {
                ALLOC.dealloc(p, huge);
            }
        }
    }

    0
}
