//! Subprocess isolation: heaps created while a subproc is current stay grouped
//! for `mi_subproc_visit_heaps`. Memory is not fully partitioned (inspired rewrite).

use crate::heap::{self, Heap};
use crate::os;
use crate::spin::SpinLock;
use crate::stats::{self, AllocStats, Stats};
use core::ptr;
#[cfg(not(target_arch = "wasm32"))]
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::{AtomicPtr, Ordering};

pub const SUBPROC_MAGIC: u32 = 0x4D495350; // 'MISP'

#[repr(C)]
pub struct Subproc {
    pub magic: u32,
    pub next_meta: *mut Subproc,
    pub stats: AllocStats,
}

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
static CURRENT_KEY: AtomicUsize = AtomicUsize::new(usize::MAX);
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
        let chunk = os::mmap_anon(META_CHUNK);
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
fn ensure_key() -> libc::pthread_key_t {
    let k = CURRENT_KEY.load(Ordering::Acquire);
    if k != usize::MAX {
        return k as libc::pthread_key_t;
    }
    let _g = LOCK.lock();
    let k = CURRENT_KEY.load(Ordering::Acquire);
    if k != usize::MAX {
        return k as libc::pthread_key_t;
    }
    let mut key: libc::pthread_key_t = 0;
    unsafe {
        if libc::pthread_key_create(&mut key, None) != 0 {
            os::abort();
        }
    }
    CURRENT_KEY.store(key as usize, Ordering::Release);
    key
}

pub unsafe fn current() -> SubprocId {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let key = ensure_key();
        let p = libc::pthread_getspecific(key) as *mut Subproc;
        if !p.is_null() && (*p).magic == SUBPROC_MAGIC {
            return SubprocId { ptr: p };
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
        let key = ensure_key();
        if libc::pthread_getspecific(key) as *mut Subproc == s {
            libc::pthread_setspecific(key, ptr::null());
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

pub unsafe fn add_current_thread(id: SubprocId) {
    let s = id.ptr;
    if s.is_null() || (*s).magic != SUBPROC_MAGIC {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let key = ensure_key();
        libc::pthread_setspecific(key, s as *const libc::c_void);
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
