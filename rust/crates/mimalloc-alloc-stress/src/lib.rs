//! Host `#[global_allocator]` stress: Vec/HashMap/threads and the `GlobalAlloc` trait.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use mimalloc_core::Mimalloc;
use std::collections::HashMap;
use std::thread;

#[global_allocator]
static ALLOC: Mimalloc = Mimalloc;

const ALIGNS: &[usize] = &[1, 2, 8, 16, 32, 64, 128, 256, 512, 1024, 4096];

/// Returns 0 on success, a non-zero probe id on failure.
pub fn run() -> i32 {
    if let Err(id) = vec_grow_shrink() {
        return id;
    }
    if let Err(id) = hashmap_roundtrip() {
        return id;
    }
    if let Err(id) = layout_sweep() {
        return id;
    }
    if let Err(id) = realloc_high_align() {
        return id;
    }
    if let Err(id) = alloc_zeroed_pages() {
        return id;
    }
    if let Err(id) = threaded_mix() {
        return id;
    }
    0
}

fn vec_grow_shrink() -> Result<(), i32> {
    let mut v = Vec::new();
    for i in 0..4096u32 {
        v.push(i);
    }
    if v.len() != 4096 || v[0] != 0 || v[4095] != 4095 {
        return Err(1);
    }
    v.reserve(64 * 1024);
    if v[100] != 100 {
        return Err(2);
    }
    let mut w = vec![0xABu8; 256 * 1024];
    if w[0] != 0xAB || w[w.len() - 1] != 0xAB {
        return Err(3);
    }
    w.truncate(64);
    w.shrink_to_fit();
    if w.len() != 64 || w[0] != 0xAB {
        return Err(4);
    }
    Ok(())
}

fn hashmap_roundtrip() -> Result<(), i32> {
    let mut m = HashMap::new();
    for i in 0..4096u32 {
        m.insert(i, i.wrapping_mul(3));
    }
    if m.len() != 4096 || m.get(&7) != Some(&21) || m.get(&4095) != Some(&(4095 * 3)) {
        return Err(10);
    }
    m.remove(&7);
    if m.contains_key(&7) {
        return Err(11);
    }
    Ok(())
}

fn layout_sweep() -> Result<(), i32> {
    for &align in ALIGNS {
        for &size in &[
            0usize,
            1,
            8,
            16,
            24,
            48,
            96,
            align,
            align.saturating_mul(3),
            4096,
        ] {
            let Ok(layout) = Layout::from_size_align(size, align) else {
                continue;
            };
            unsafe {
                let p = ALLOC.alloc(layout);
                if p.is_null() {
                    return Err(20);
                }
                if (p as usize) % align != 0 {
                    ALLOC.dealloc(p, layout);
                    return Err(21);
                }
                if size != 0 {
                    ptr::write_bytes(p, 0x5A, size);
                    if *p != 0x5A {
                        ALLOC.dealloc(p, layout);
                        return Err(22);
                    }
                }
                ALLOC.dealloc(p, layout);
            }
        }
    }
    Ok(())
}

fn realloc_high_align() -> Result<(), i32> {
    let layout = Layout::from_size_align(48, 64).map_err(|_| 30)?;
    unsafe {
        let p = ALLOC.alloc(layout);
        if p.is_null() || (p as usize) % 64 != 0 {
            return Err(31);
        }
        ptr::write_bytes(p, 0xEF, 48);
        let q = ALLOC.realloc(p, layout, 4096);
        if q.is_null() || (q as usize) % 64 != 0 || *q != 0xEF {
            if !q.is_null() {
                ALLOC.dealloc(q, Layout::from_size_align(4096, 64).unwrap());
            }
            return Err(32);
        }
        let r = ALLOC.realloc(q, Layout::from_size_align(4096, 64).unwrap(), 16);
        if r.is_null() || (r as usize) % 64 != 0 || *r != 0xEF {
            if !r.is_null() {
                ALLOC.dealloc(r, Layout::from_size_align(16, 64).unwrap());
            }
            return Err(33);
        }
        ALLOC.dealloc(r, Layout::from_size_align(16, 64).unwrap());
    }
    Ok(())
}

fn alloc_zeroed_pages() -> Result<(), i32> {
    let layout = Layout::from_size_align(8192, 4096).map_err(|_| 40)?;
    unsafe {
        let p = ALLOC.alloc_zeroed(layout);
        if p.is_null() || (p as usize) % 4096 != 0 {
            return Err(41);
        }
        for i in 0..8192 {
            if *p.add(i) != 0 {
                ALLOC.dealloc(p, layout);
                return Err(42);
            }
        }
        *p.add(100) = 1;
        ALLOC.dealloc(p, layout);
    }
    Ok(())
}

fn threaded_mix() -> Result<(), i32> {
    let mut handles = Vec::new();
    for t in 0..8u32 {
        handles.push(thread::spawn(move || {
            let mut local = Vec::new();
            let mut map = HashMap::new();
            for i in 0..1500u32 {
                let n = ((i + t) % 512) as usize + 1;
                let layout = Layout::from_size_align(n, 8).unwrap();
                unsafe {
                    let p = ALLOC.alloc(layout);
                    if p.is_null() || (p as usize) % 8 != 0 {
                        return 50;
                    }
                    ptr::write(p, (i + t) as u8);
                    local.push((p, layout, (i + t) as u8));
                }
                map.insert(i, i);
            }
            if map.len() != 1500 {
                return 51;
            }
            for (p, layout, b) in local {
                unsafe {
                    if *p != b {
                        ALLOC.dealloc(p, layout);
                        return 52;
                    }
                    ALLOC.dealloc(p, layout);
                }
            }
            0
        }));
    }
    for h in handles {
        match h.join() {
            Ok(0) => {}
            Ok(id) => return Err(id),
            Err(_) => return Err(59),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_alloc_stress() {
        assert_eq!(run(), 0);
    }
}
