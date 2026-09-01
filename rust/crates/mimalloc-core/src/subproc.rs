//! Subprocess isolation (C `subproc.c`).
//!
//! C keeps fully separate arenas per subproc (e.g. multiple Python
//! interpreters). This rewrite groups heaps created while a subproc is
//! current so `mi_subproc_visit_heaps` works; OS memory is not partitioned.
//! The main subproc is created lazily on first use.

use crate::heap::{self, Heap};
use crate::os;
use crate::spin::SpinLock;
use crate::stats::{self, AllocStats, Stats};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, Ordering};

pub const SUBPROC_MAGIC: u32 = 0x4D495350; // 'MISP'

#[repr(C)]
pub struct Subproc {
    pub magic: u32,
    pub next_meta: *mut Subproc,
    pub stats: AllocStats,
}

/// Opaque id matching C `mi_subproc_id_t` (a pointer).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SubprocId {
    pub ptr: *mut Subproc,
}

static LOCK: SpinLock = SpinLock::new();
static mut META_BUMP: *mut u8 = ptr::null_mut();
static mut META_END: *mut u8 = ptr::null_mut();
static mut META_FREE: *mut Subproc = ptr::null_mut();
static MAIN: AtomicPtr<Subproc> = AtomicPtr::new(ptr::null_mut());
#[cfg(not(target_arch = "wasm32"))]
static CURRENT_SLOT: crate::os::TlsSlot = crate::os::TlsSlot::new();
#[cfg(target_arch = "wasm32")]
static CURRENT: AtomicPtr<Subproc> = AtomicPtr::new(ptr::null_mut());
const META_CHUNK: usize = 64 * 1024;

unsafe fn meta_alloc() -> *mut Subproc {
    let _g = LOCK.lock();
    if !META_FREE.is_null() {
        let s = META_FREE;
        META_FREE = (*s).next_meta;
        ptr::write_bytes(s as *mut u8, 0, core::mem::size_of::<Subproc>());
        return s;
    }
    let need = core::mem::size_of::<Subproc>();
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
    let s = META_BUMP as *mut Subproc;
    META_BUMP = META_BUMP.add(need);
    ptr::write_bytes(s as *mut u8, 0, need);
    s
}

unsafe fn meta_free(s: *mut Subproc) {
    if s.is_null() {
        return;
    }
    let _g = LOCK.lock();
    (*s).next_meta = META_FREE;
    META_FREE = s;
}

/// Process-wide main subproc (`mi_subproc_main`).
pub unsafe fn main() -> SubprocId {
    crate::init();
    let existing = MAIN.load(Ordering::Acquire);
    if !existing.is_null() {
        return SubprocId { ptr: existing };
    }
    let s = meta_alloc();
    if s.is_null() {
        return SubprocId {
            ptr: ptr::null_mut(),
        };
    }
    (*s).magic = SUBPROC_MAGIC;
    match MAIN.compare_exchange(ptr::null_mut(), s, Ordering::Release, Ordering::Acquire) {
        Ok(_) => SubprocId { ptr: s },
        Err(cur) => {
            meta_free(s);
            SubprocId { ptr: cur }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_key() {
    if CURRENT_SLOT.is_ready() {
        return;
    }
    let _g = LOCK.lock();
    if CURRENT_SLOT.is_ready() {
        return;
    }
    crate::tls::IN_BOOTSTRAP.store(true, Ordering::Release);
    crate::tls::BOOTSTRAP_TID.store(os::thread_id(), Ordering::Release);
    CURRENT_SLOT.ensure(None);
    crate::tls::BOOTSTRAP_TID.store(0, Ordering::Release);
    crate::tls::IN_BOOTSTRAP.store(false, Ordering::Release);
}

/// Subproc of this thread, or [`main`] if none was set.
pub unsafe fn current() -> SubprocId {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if CURRENT_SLOT.is_ready() {
            if let Some(p) = CURRENT_SLOT.get_non_null::<Subproc>() {
                if (*p.as_ptr()).magic == SUBPROC_MAGIC {
                    return SubprocId { ptr: p.as_ptr() };
                }
            }
        }
        return main();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let p = CURRENT.load(Ordering::Acquire);
        if !p.is_null() && (*p).magic == SUBPROC_MAGIC {
            return SubprocId { ptr: p };
        }
        main()
    }
}

pub unsafe fn current_ptr() -> *mut Subproc {
    current().ptr
}

/// `mi_subproc_new`.
pub unsafe fn new() -> SubprocId {
    crate::init();
    let s = meta_alloc();
    if s.is_null() {
        return SubprocId {
            ptr: ptr::null_mut(),
        };
    }
    (*s).magic = SUBPROC_MAGIC;
    SubprocId { ptr: s }
}

/// Destroy heaps/arenas tagged with `id`, then recycle the object.
pub unsafe fn destroy(id: SubprocId) {
    let s = id.ptr;
    if s.is_null() || (*s).magic != SUBPROC_MAGIC {
        return;
    }
    if s == MAIN.load(Ordering::Acquire) {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ensure_key();
        let cur = CURRENT_SLOT
            .get_non_null::<Subproc>()
            .map(|n| n.as_ptr())
            .unwrap_or(ptr::null_mut());
        if cur == s {
            CURRENT_SLOT.set_non_null::<Subproc>(None);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        if CURRENT.load(Ordering::Acquire) == s {
            CURRENT.store(ptr::null_mut(), Ordering::Release);
        }
    }
    heap::destroy_heaps_in_subproc(s);
    crate::arena::destroy_owned_in_subproc(s);
    (*s).magic = 0;
    meta_free(s);
}

/// Bind this thread to `id` (`mi_subproc_add_current_thread`).
pub unsafe fn add_current_thread(id: SubprocId) {
    let s = id.ptr;
    if s.is_null() || (*s).magic != SUBPROC_MAGIC {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ensure_key();
        CURRENT_SLOT.set_non_null(NonNull::new(s));
    }
    #[cfg(target_arch = "wasm32")]
    {
        CURRENT.store(s, Ordering::Release);
    }
}

pub fn is_valid(s: *const Subproc) -> bool {
    !s.is_null() && unsafe { (*s).magic == SUBPROC_MAGIC }
}

pub type HeapVisitFun = unsafe extern "C" fn(heap: *mut Heap, arg: *mut core::ffi::c_void) -> bool;

pub unsafe fn visit_heaps(
    id: SubprocId,
    visitor: Option<HeapVisitFun>,
    arg: *mut core::ffi::c_void,
) -> bool {
    let Some(visitor) = visitor else {
        return true;
    };
    let s = id.ptr;
    if !is_valid(s) {
        return false;
    }
    heap::visit_all_heaps(s, visitor, arg)
}

unsafe extern "C" fn add_heap_stats(h: *mut Heap, arg: *mut core::ffi::c_void) -> bool {
    heap::heap_stats_add_into(h, arg as *mut Stats);
    true
}

pub unsafe fn stats_get(id: SubprocId, out: *mut Stats, exclusive: bool) -> bool {
    let s = id.ptr;
    if out.is_null() || !is_valid(s) {
        return false;
    }
    stats::clear(out);
    (*s).stats.copy_into(out);
    if !exclusive {
        heap::visit_all_heaps(s, add_heap_stats, out as *mut core::ffi::c_void);
    }
    true
}

pub fn force_unlock() {
    unsafe {
        LOCK.force_unlock();
    }
}
