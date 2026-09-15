//! Experimental sampled alloc/free hooks (`mimalloc-profile.h`).
//!
//! Sample metadata lives in a side table keyed by the user pointer so free
//! lists stay encoded. Default: no profiler attached.

use crate::heap::{self, Heap, ThreadHeap};
use crate::os;
use crate::spin::SpinLock;
use crate::subproc::SubprocId;
use core::ptr::{self, addr_of_mut};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub const SAMPLE_DATA_MAX: usize = 1024;
pub const SAMPLE_RATE_MAX: usize = usize::MAX / 4;

/// Layout-compatible with C `mi_profiler_sample_data_t` (flexible array).
#[repr(C)]
pub struct ProfilerSampleData {
    pub user_data_size: usize,
    pub user_data: [*mut u8; 1],
}

/// Layout-compatible with C `mi_profiler_t`.
#[repr(C)]
pub struct Profiler {
    pub reserved: usize,
    pub sample_data_size: usize,
    pub initial_sample_rate: usize,
    pub on_alloc: Option<
        unsafe extern "C" fn(
            *mut Profiler,
            *mut ProfilerSampleData,
            *mut u8,
            usize,
            usize,
            u64,
            *const Heap,
        ) -> usize,
    >,
    pub on_free:
        Option<unsafe extern "C" fn(*mut Profiler, *mut ProfilerSampleData, *mut u8, *const Heap)>,
    pub on_realloc_inplace: Option<
        unsafe extern "C" fn(
            *mut Profiler,
            *mut ProfilerSampleData,
            *mut u8,
            usize,
            *const Heap,
        ) -> usize,
    >,
}

#[repr(C)]
struct SampleNode {
    ptr: usize,
    next: *mut SampleNode,
    data: ProfilerSampleData,
    extra: [u8; SAMPLE_DATA_MAX],
}

static SAMPLES: SpinLock = SpinLock::new();
static mut SAMPLE_HEAD: *mut SampleNode = ptr::null_mut();
static mut SAMPLE_BUMP: *mut u8 = ptr::null_mut();
static mut SAMPLE_END: *mut u8 = ptr::null_mut();
static SAMPLE_OWNER: AtomicU32 = AtomicU32::new(0);
const SAMPLE_CHUNK: usize = 64 * 1024;

static NOSAMPLE: Profiler = Profiler {
    reserved: 0,
    sample_data_size: 0,
    initial_sample_rate: SAMPLE_RATE_MAX,
    on_alloc: None,
    on_free: None,
    on_realloc_inplace: None,
};

unsafe fn enabled_flag(prof: *mut Profiler) -> *mut AtomicUsize {
    addr_of_mut!((*prof).reserved) as *mut AtomicUsize
}

unsafe fn is_enabled(prof: *mut Profiler) -> bool {
    if prof.is_null() {
        return false;
    }
    (*enabled_flag(prof)).load(Ordering::Acquire) != 0
}

unsafe fn set_enabled(prof: *mut Profiler, enable: bool) -> bool {
    if prof.is_null() {
        return false;
    }
    (*enabled_flag(prof)).swap(if enable { 1 } else { 0 }, Ordering::Release) != 0
}

unsafe fn alloc_node() -> *mut SampleNode {
    let need = core::mem::size_of::<SampleNode>();
    if SAMPLE_BUMP.is_null() || SAMPLE_BUMP.add(need) > SAMPLE_END {
        let chunk = os::Mapping::anon(SAMPLE_CHUNK)
            .map(|m| m.leak())
            .unwrap_or(ptr::null_mut());
        if chunk.is_null() {
            return ptr::null_mut();
        }
        SAMPLE_BUMP = chunk;
        SAMPLE_END = chunk.add(SAMPLE_CHUNK);
    }
    let n = SAMPLE_BUMP as *mut SampleNode;
    SAMPLE_BUMP = SAMPLE_BUMP.add(need);
    ptr::write_bytes(n as *mut u8, 0, need);
    n
}

fn profiler_of(th: *mut ThreadHeap) -> *mut Profiler {
    unsafe {
        if th.is_null() {
            return ptr::null_mut();
        }
        let owner = if (*th).owner.is_null() {
            heap::heap_main()
        } else {
            (*th).owner
        };
        if owner.is_null() {
            return ptr::null_mut();
        }
        (*owner).profiler.load(Ordering::Acquire)
    }
}

unsafe fn enabled_profiler(th: *mut ThreadHeap) -> *mut Profiler {
    let prof = profiler_of(th);
    if prof.is_null() || !is_enabled(prof) || (*prof).on_alloc.is_none() {
        ptr::null_mut()
    } else {
        prof
    }
}

unsafe fn set_theap_rate(th: *mut ThreadHeap, rate: usize) {
    if th.is_null() {
        return;
    }
    let rate = rate.min(SAMPLE_RATE_MAX);
    (*th).profile_sample_rate = rate;
    if (*th).profile_countdown > rate || (*th).profile_countdown == 0 {
        (*th).profile_countdown = rate;
    }
}

/// After a successful user allocation: maybe fire `on_alloc`.
pub unsafe fn on_malloc(th: *mut ThreadHeap, p: *mut u8, req_size: usize) {
    if p.is_null() || th.is_null() || crate::tls::in_recursive_setup() {
        return;
    }
    let me = crate::os::thread_id();
    if SAMPLE_OWNER.load(Ordering::Acquire) == me {
        return;
    }
    let prof = enabled_profiler(th);
    if prof.is_null() {
        return;
    }
    if (*th).profile_sample_rate == 0 {
        set_theap_rate(th, (*prof).initial_sample_rate.max(1));
    }
    let size = req_size.max(1);
    let cd = (*th).profile_countdown;
    if cd > size {
        (*th).profile_countdown = cd - size;
        (*th).sample_requested = (*th).sample_requested.wrapping_add(size as u64);
        return;
    }
    let requested = (*th).sample_requested.wrapping_add(
        ((*th)
            .profile_sample_rate
            .saturating_add(size.saturating_sub(cd))) as u64,
    );
    (*th).sample_requested = 0;
    (*th).profile_countdown = (*th).profile_sample_rate.max(1);

    SAMPLE_OWNER.store(me, Ordering::Release);
    let data_ptr = if (*prof).on_free.is_some() && (*prof).sample_data_size != 0 {
        let _g = SAMPLES.lock();
        let node = alloc_node();
        if node.is_null() {
            SAMPLE_OWNER.store(0, Ordering::Release);
            return;
        }
        (*node).ptr = p as usize;
        (*node).next = SAMPLE_HEAD;
        SAMPLE_HEAD = node;
        let user = (*prof).sample_data_size.min(SAMPLE_DATA_MAX);
        (*node).data.user_data_size = user;
        &mut (*node).data
    } else {
        ptr::null_mut()
    };
    let heap = if (*th).owner.is_null() {
        heap::heap_main()
    } else {
        (*th).owner
    };
    let new_rate = if let Some(cb) = (*prof).on_alloc {
        cb(
            prof,
            data_ptr,
            p,
            req_size,
            (*th).profile_sample_rate,
            requested.max(size as u64),
            heap,
        )
    } else {
        0
    };
    SAMPLE_OWNER.store(0, Ordering::Release);
    if new_rate != 0 && new_rate != (*th).profile_sample_rate {
        set_theap_rate(th, new_rate);
    }
    crate::stats::profile_sample_add();
}

/// Before recycling a user pointer: maybe fire `on_free`.
pub unsafe fn on_free(p: *mut u8) {
    if p.is_null() {
        return;
    }
    let me = crate::os::thread_id();
    if SAMPLE_OWNER.load(Ordering::Acquire) == me {
        return;
    }
    let key = p as usize;
    let _g = SAMPLES.lock();
    let mut prev: *mut SampleNode = ptr::null_mut();
    let mut cur = SAMPLE_HEAD;
    while !cur.is_null() {
        let next = (*cur).next;
        if (*cur).ptr == key {
            if prev.is_null() {
                SAMPLE_HEAD = next;
            } else {
                (*prev).next = next;
            }
            let data = &mut (*cur).data as *mut ProfilerSampleData;
            drop(_g);
            SAMPLE_OWNER.store(me, Ordering::Release);
            let heap = heap::heap_of(p);
            let prof = if heap.is_null() {
                ptr::null_mut()
            } else {
                (*heap).profiler.load(Ordering::Acquire)
            };
            if !prof.is_null() && is_enabled(prof) {
                if let Some(cb) = (*prof).on_free {
                    cb(prof, data, p, heap);
                }
            }
            SAMPLE_OWNER.store(0, Ordering::Release);
            return;
        }
        prev = cur;
        cur = next;
    }
}

unsafe fn heap_set_profiler(h: *mut Heap, profiler: *mut Profiler) -> bool {
    if h.is_null() || (*h).magic != heap::HEAP_MAGIC {
        return false;
    }
    let previous = if profiler.is_null() {
        (*h).profiler.load(Ordering::Acquire)
    } else {
        ptr::null_mut()
    };
    (*h).profiler
        .compare_exchange(previous, profiler, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// `mi_heap_profile`.
pub unsafe fn heap_profile(h: *mut Heap, profiler: *mut Profiler) -> bool {
    crate::init();
    let h = if h.is_null() { heap::heap_main() } else { h };
    profiler_stop(profiler);
    heap_set_profiler(h, profiler)
}

/// `mi_heap_profile_disable`.
pub unsafe fn heap_profile_disable(h: *mut Heap) {
    crate::init();
    let h = if h.is_null() { heap::heap_main() } else { h };
    heap_set_profiler(h, ptr::null_mut());
    let _ = heap_set_profiler(h, &NOSAMPLE as *const Profiler as *mut Profiler);
}

unsafe extern "C" fn set_heap_profiler(h: *mut Heap, arg: *mut core::ffi::c_void) -> bool {
    let _ = heap_set_profiler(h, arg as *mut Profiler);
    true
}

/// `mi_subproc_profile`.
pub unsafe fn subproc_profile(id: SubprocId, profiler: *mut Profiler) -> bool {
    crate::init();
    let s = id.ptr;
    if s.is_null() || (*s).magic != crate::subproc::SUBPROC_MAGIC {
        return false;
    }
    profiler_stop(profiler);
    let previous = if profiler.is_null() {
        (*s).profiler.load(Ordering::Acquire)
    } else {
        ptr::null_mut()
    };
    if (*s)
        .profiler
        .compare_exchange(previous, profiler, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    heap::visit_all_heaps(s, set_heap_profiler, profiler as *mut core::ffi::c_void);
    true
}

/// `mi_profile` (main subprocess).
pub unsafe fn profile(profiler: *mut Profiler) -> bool {
    subproc_profile(crate::subproc::main(), profiler)
}

/// `mi_profiler_start`. Returns whether it was already running (C ABI).
pub unsafe fn profiler_start(profiler: *mut Profiler) -> bool {
    if profiler.is_null() {
        return false;
    }
    let was = set_enabled(profiler, true);
    if was {
        return true;
    }
    let heap = heap::heap_main();
    if !heap.is_null() && (*heap).profiler.load(Ordering::Acquire) == profiler {
        let th = crate::tls::default_theap();
        if !th.is_null() {
            set_theap_rate(th, 1);
        }
    }
    false
}

/// `mi_profiler_stop`.
pub unsafe fn profiler_stop(profiler: *mut Profiler) -> bool {
    if profiler.is_null() {
        return true;
    }
    set_enabled(profiler, false)
}

/// Inherit a subprocess profiler onto a newly created heap.
pub unsafe fn inherit_from_subproc(h: *mut Heap) {
    if h.is_null() || (*h).subproc.is_null() {
        return;
    }
    let p = (*(*h).subproc).profiler.load(Ordering::Acquire);
    if !p.is_null() {
        (*h).profiler.store(p, Ordering::Release);
    }
}
