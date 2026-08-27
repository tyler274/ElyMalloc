//! Reserved OS memory regions. Heaps created with `heap_new_in_arena` bump
//! allocate pages from an exclusive arena instead of calling `mmap` per page.

use crate::os;
use crate::spin::SpinLock;
use crate::{align_up, SLICE_SIZE};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

pub const ARENA_MAGIC: u32 = 0x4D494152; // 'MIAR'
pub const ARENA_MIN_SIZE: usize = 32 * 1024 * 1024;
pub const ARENA_MIN_ALIGN: usize = SLICE_SIZE;

#[repr(C)]
pub struct Arena {
    pub magic: u32,
    pub exclusive: bool,
    pub committed: bool,
    pub base: *mut u8,
    pub size: usize,
    bump: AtomicUsize,
    next: *mut Arena,
}

static LOCK: SpinLock = SpinLock::new();
static LIST: AtomicPtr<Arena> = AtomicPtr::new(ptr::null_mut());
static mut META_BUMP: *mut u8 = ptr::null_mut();
static mut META_END: *mut u8 = ptr::null_mut();

const META_CHUNK: usize = 64 * 1024;

unsafe fn meta_alloc() -> *mut Arena {
    let _g = LOCK.lock();
    let need = core::mem::size_of::<Arena>();
    if META_BUMP.is_null() || META_BUMP.add(need) > META_END {
        let chunk = os::mmap_anon(META_CHUNK);
        if chunk.is_null() {
            return ptr::null_mut();
        }
        META_BUMP = chunk;
        META_END = chunk.add(META_CHUNK);
    }
    let a = META_BUMP as *mut Arena;
    META_BUMP = META_BUMP.add(need);
    ptr::write_bytes(a as *mut u8, 0, need);
    a
}

#[inline]
pub fn is_valid(a: *const Arena) -> bool {
    !a.is_null() && unsafe { (*a).magic == ARENA_MAGIC }
}

pub unsafe fn reserve(
    size: usize,
    commit: bool,
    _allow_large: bool,
    exclusive: bool,
) -> *mut Arena {
    crate::init();
    if size == 0 {
        return ptr::null_mut();
    }
    let size = align_up(size.max(SLICE_SIZE), SLICE_SIZE);
    let extra = if commit { 0 } else { libc::MAP_NORESERVE };
    let prot = if commit {
        libc::PROT_READ | libc::PROT_WRITE
    } else {
        libc::PROT_NONE
    };
    let base = os::mmap_aligned_prot(size, SLICE_SIZE, prot, extra);
    if base.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    let a = meta_alloc();
    if a.is_null() {
        os::munmap(base, size);
        os::enomem();
        return ptr::null_mut();
    }
    (*a).magic = ARENA_MAGIC;
    (*a).exclusive = exclusive;
    (*a).committed = commit;
    (*a).base = base;
    (*a).size = size;
    (*a).bump = AtomicUsize::new(0);
    loop {
        let old = LIST.load(Ordering::Acquire);
        (*a).next = old;
        if LIST
            .compare_exchange_weak(old, a, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
    a
}

/// Bump-allocate `size` bytes at `align` from `arena`. Both must be slice-aligned
/// in practice so the page map can key off 64 KiB slices.
pub unsafe fn alloc(arena: *mut Arena, size: usize, align: usize) -> *mut u8 {
    if !is_valid(arena) || size == 0 {
        return ptr::null_mut();
    }
    let align = align.max(1);
    if !align.is_power_of_two() {
        return ptr::null_mut();
    }
    let size = align_up(size, SLICE_SIZE.max(os::page_size()));
    loop {
        let pos = (*arena).bump.load(Ordering::Relaxed);
        let aligned = align_up(pos, align);
        let Some(new_pos) = aligned.checked_add(size) else {
            return ptr::null_mut();
        };
        if new_pos > (*arena).size {
            return ptr::null_mut();
        }
        if (*arena)
            .bump
            .compare_exchange_weak(pos, new_pos, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            let p = (*arena).base.add(aligned);
            if !(*arena).committed {
                let _ = libc::mprotect(
                    p as *mut libc::c_void,
                    size,
                    libc::PROT_READ | libc::PROT_WRITE,
                );
            }
            return p;
        }
    }
}

pub fn contains(arena: *const Arena, p: *const u8) -> bool {
    if !is_valid(arena) || p.is_null() {
        return false;
    }
    unsafe {
        let b = (*arena).base as usize;
        let addr = p as usize;
        addr >= b && addr < b.wrapping_add((*arena).size)
    }
}

pub fn area(arena: *const Arena, size_out: *mut usize) -> *mut u8 {
    if !is_valid(arena) {
        if !size_out.is_null() {
            unsafe {
                *size_out = 0;
            }
        }
        return ptr::null_mut();
    }
    unsafe {
        if !size_out.is_null() {
            *size_out = (*arena).size;
        }
        (*arena).base
    }
}

/// Adopt an existing mapping as an arena (does not take ownership / munmap it).
pub unsafe fn manage(
    start: *mut u8,
    size: usize,
    is_committed: bool,
    _is_pinned: bool,
    _is_zero: bool,
    _numa_node: i32,
    exclusive: bool,
) -> *mut Arena {
    crate::init();
    if start.is_null() || size < SLICE_SIZE {
        return ptr::null_mut();
    }
    let addr = start as usize;
    let aligned = align_up(addr, SLICE_SIZE);
    if aligned < addr {
        return ptr::null_mut();
    }
    let skip = aligned - addr;
    if size <= skip + SLICE_SIZE {
        return ptr::null_mut();
    }
    let usable = (size - skip) & !(SLICE_SIZE - 1);
    if usable < SLICE_SIZE {
        return ptr::null_mut();
    }
    let a = meta_alloc();
    if a.is_null() {
        return ptr::null_mut();
    }
    (*a).magic = ARENA_MAGIC;
    (*a).exclusive = exclusive;
    (*a).committed = is_committed;
    (*a).base = aligned as *mut u8;
    (*a).size = usable;
    (*a).bump = AtomicUsize::new(0);
    loop {
        let old = LIST.load(Ordering::Acquire);
        (*a).next = old;
        if LIST
            .compare_exchange_weak(old, a, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
    a
}

pub fn force_unlock() {
    unsafe {
        LOCK.force_unlock();
    }
}
