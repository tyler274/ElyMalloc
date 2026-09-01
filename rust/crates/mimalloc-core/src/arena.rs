//! Reserved OS regions (C `arena.c`).
//!
//! C arenas use an atomic bitmap of 64 KiB slices and are shared across
//! threads. This rewrite bump-allocates from an exclusive arena when a heap
//! is created with [`heap_new_in_arena`](crate::heap_new_in_arena); otherwise
//! pages call `mmap` directly. Adopted mappings (`manage`) are not `munmap`'d.
//!
//! Arenas are process-shared metadata; the bump is atomic. Slice alignment
//! is required so the page map can key each 64 KiB.

use crate::os;
use crate::spin::SpinLock;
use crate::{align_up, SLICE_SIZE};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

pub const ARENA_MAGIC: u32 = 0x4D494152; // 'MIAR'
pub const ARENA_MIN_SIZE: usize = 32 * 1024 * 1024;
pub const ARENA_MIN_ALIGN: usize = SLICE_SIZE;

/// Fixed OS reservation from which exclusive heaps take slices.
#[repr(C)]
pub struct Arena {
    pub magic: u32,
    /// Heaps created in this arena must not fall back to OS `mmap`.
    pub exclusive: bool,
    pub committed: bool,
    /// True if we `mmap`'d `base` and should `munmap` on destroy.
    pub owned: bool,
    pub base: *mut u8,
    pub size: usize,
    pub subproc: *mut crate::subproc::Subproc,
    pub numa_node: i32,
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
        let chunk = os::Mapping::anon(META_CHUNK)
            .map(|m| m.leak())
            .unwrap_or(ptr::null_mut());
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

/// `mi_reserve_os_memory_ex`: `mmap` a new owned arena.
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
    let extra = if commit { 0 } else { os::MAP_NORESERVE };
    let prot = if commit {
        os::PROT_READ | os::PROT_WRITE
    } else {
        os::PROT_NONE
    };
    let Some(map) = os::Mapping::aligned_prot(size, SLICE_SIZE, prot, extra) else {
        os::enomem();
        return ptr::null_mut();
    };
    let a = meta_alloc();
    if a.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    let base = map.leak();
    (*a).magic = ARENA_MAGIC;
    (*a).exclusive = exclusive;
    (*a).committed = commit;
    (*a).owned = true;
    (*a).base = base;
    (*a).size = size;
    (*a).subproc = crate::subproc::current_ptr();
    (*a).numa_node = -1;
    (*a).bump = AtomicUsize::new(0);
    crate::stats::arena_add();
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
                let _ = os::commit(p, size);
                crate::stats::commit_add(size);
            }
            return p;
        }
    }
}

/// True if `p` lies in `[base, base+size)`.
pub fn contains(arena: *const Arena, p: *const u8) -> bool {
    if !is_valid(arena) || p.is_null() {
        return false;
    }
    unsafe {
        let b = crate::ptrx::addr((*arena).base);
        let addr = crate::ptrx::addr(p);
        addr >= b && addr < b.wrapping_add((*arena).size)
    }
}

/// Base pointer and size (`mi_arena_area`).
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
    numa_node: i32,
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
    (*a).owned = false;
    (*a).base = aligned as *mut u8;
    (*a).size = usable;
    (*a).subproc = crate::subproc::current_ptr();
    (*a).numa_node = if numa_node < 0 { -1 } else { numa_node };
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

pub type ArenaVisitFun =
    unsafe extern "C" fn(arena: *mut Arena, arg: *mut core::ffi::c_void) -> bool;

pub unsafe fn visit_all(visitor: ArenaVisitFun, arg: *mut core::ffi::c_void) -> bool {
    let mut cur = LIST.load(Ordering::Acquire);
    while !cur.is_null() {
        let next = (*cur).next;
        if (*cur).magic == ARENA_MAGIC && !visitor(cur, arg) {
            return false;
        }
        cur = next;
    }
    true
}

pub fn force_unlock() {
    unsafe {
        LOCK.force_unlock();
    }
}

/// Unmap arenas we own that were created in `subproc` (not `manage` adoptions).
pub unsafe fn destroy_owned_in_subproc(s: *mut crate::subproc::Subproc) {
    if s.is_null() {
        return;
    }
    let mut cur = LIST.load(Ordering::Acquire);
    while !cur.is_null() {
        let next = (*cur).next;
        if (*cur).magic == ARENA_MAGIC && (*cur).owned && (*cur).subproc == s {
            (*cur).magic = 0;
            os::munmap((*cur).base, (*cur).size);
        }
        cur = next;
    }
}
