//! Thread-local heap: pthread keys on Linux, a process heap on wasm32.
//!
//! Two keys: the thread's default theap (destructor abandons pages) and
//! the current default used by `malloc` (`mi_heap_set_default`). During
//! `pthread_key_create` / `pthread_setspecific` / first `heap::create`,
//! [`IN_BOOTSTRAP`] is set so malloc uses a static bump (glibc may allocate
//! from those paths). A tid slot marks the creating thread so recursive
//! malloc cannot call `heap::create` again (that used to mmap until OOM).

use crate::heap;
use core::sync::atomic::{AtomicBool, AtomicU32};

/// True while creating the pthread key or a thread heap; [`alloc::malloc`](crate::alloc::malloc) uses the bump.
pub static IN_BOOTSTRAP: AtomicBool = AtomicBool::new(false);
/// Thread that is in bootstrap; other threads spin until [`crate::init`] finishes.
pub static BOOTSTRAP_TID: AtomicU32 = AtomicU32::new(0);
/// Thread holding [`crate::init`]'s lock (nested malloc must not take it).
pub static INIT_OWNER: AtomicU32 = AtomicU32::new(0);

#[cfg(not(target_arch = "wasm32"))]
mod pthread {
    use super::*;
    use crate::heap::ThreadHeap;
    use crate::spin::SpinLock;
    use core::ptr;
    use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    const KEY_UNSET: usize = usize::MAX;
    const CREATE_SLOTS: usize = 1024;

    static KEY: AtomicUsize = AtomicUsize::new(KEY_UNSET);
    static DEFAULT_KEY: AtomicUsize = AtomicUsize::new(KEY_UNSET);
    static KEY_LOCK: SpinLock = SpinLock::new();
    static CREATING_TID: [AtomicU32; CREATE_SLOTS] = {
        const ZERO: AtomicU32 = AtomicU32::new(0);
        [ZERO; CREATE_SLOTS]
    };

    fn create_slot(tid: u32) -> usize {
        (tid as usize).wrapping_mul(0x9E37_79B9) % CREATE_SLOTS
    }

    fn mark_creating(tid: u32) {
        CREATING_TID[create_slot(tid)].store(tid, Ordering::Release);
    }

    fn unmark_creating(tid: u32) {
        let slot = &CREATING_TID[create_slot(tid)];
        let _ = slot.compare_exchange(tid, 0, Ordering::Release, Ordering::Relaxed);
    }

    fn is_creating(tid: u32) -> bool {
        CREATING_TID[create_slot(tid)].load(Ordering::Acquire) == tid
    }

    /// True if this thread must not re-enter heap/TLS init (use the bootstrap bump).
    pub fn in_recursive_setup() -> bool {
        let me = crate::os::gettid();
        INIT_OWNER.load(Ordering::Acquire) == me
            || (IN_BOOTSTRAP.load(Ordering::Acquire) && BOOTSTRAP_TID.load(Ordering::Acquire) == me)
            || is_creating(me)
    }

    fn begin_bootstrap() {
        let me = crate::os::gettid();
        BOOTSTRAP_TID.store(me, Ordering::Release);
        IN_BOOTSTRAP.store(true, Ordering::Release);
        mark_creating(me);
    }

    fn end_bootstrap() {
        let me = crate::os::gettid();
        unmark_creating(me);
        IN_BOOTSTRAP.store(false, Ordering::Release);
        BOOTSTRAP_TID.store(0, Ordering::Release);
    }

    unsafe extern "C" fn thread_dtor(ptr: *mut libc::c_void) {
        let h = ptr as *mut ThreadHeap;
        if h.is_null() || (h as usize) < 64 {
            return;
        }
        heap::abandon(h);
    }

    fn ensure_key() -> libc::pthread_key_t {
        let k = KEY.load(Ordering::Acquire);
        if k != KEY_UNSET {
            return k as libc::pthread_key_t;
        }
        let _g = KEY_LOCK.lock();
        let k = KEY.load(Ordering::Acquire);
        if k != KEY_UNSET {
            return k as libc::pthread_key_t;
        }
        let mut key: libc::pthread_key_t = 0;
        begin_bootstrap();
        unsafe {
            let rc = libc::pthread_key_create(&mut key, Some(thread_dtor));
            end_bootstrap();
            if rc != 0 {
                crate::os::abort();
            }
        }
        KEY.store(key as usize, Ordering::Release);
        key
    }

    fn ensure_default_key() -> libc::pthread_key_t {
        let k = DEFAULT_KEY.load(Ordering::Acquire);
        if k != KEY_UNSET {
            return k as libc::pthread_key_t;
        }
        let _g = KEY_LOCK.lock();
        let k = DEFAULT_KEY.load(Ordering::Acquire);
        if k != KEY_UNSET {
            return k as libc::pthread_key_t;
        }
        let mut key: libc::pthread_key_t = 0;
        begin_bootstrap();
        unsafe {
            let rc = libc::pthread_key_create(&mut key, None);
            end_bootstrap();
            if rc != 0 {
                crate::os::abort();
            }
        }
        DEFAULT_KEY.store(key as usize, Ordering::Release);
        key
    }

    /// Thread's owning theap; the pthread destructor calls `heap::abandon`.
    pub unsafe fn thread_heap() -> *mut ThreadHeap {
        let me = crate::os::gettid();
        if is_creating(me) {
            // Recursive malloc must take the bootstrap path in `alloc::malloc`.
            return ptr::null_mut();
        }
        let key = ensure_key();
        let mut h = libc::pthread_getspecific(key) as *mut ThreadHeap;
        if h.is_null() {
            mark_creating(me);
            h = heap::create();
            if !h.is_null() {
                libc::pthread_setspecific(key, h as *const libc::c_void);
            }
            unmark_creating(me);
        }
        h
    }

    /// Current default theap for `malloc` (`mi_heap_get_default`).
    pub unsafe fn default_theap() -> *mut ThreadHeap {
        let k = DEFAULT_KEY.load(Ordering::Acquire);
        if k != KEY_UNSET {
            let t = libc::pthread_getspecific(k as libc::pthread_key_t) as *mut ThreadHeap;
            if !t.is_null() {
                return t;
            }
        } else {
            let key = ensure_default_key();
            let t = libc::pthread_getspecific(key) as *mut ThreadHeap;
            if !t.is_null() {
                return t;
            }
        }
        thread_heap()
    }

    pub unsafe fn set_default_theap(theap: *mut ThreadHeap) -> *mut ThreadHeap {
        let old = default_theap();
        let key = ensure_default_key();
        libc::pthread_setspecific(key, theap as *const libc::c_void);
        old
    }

    pub unsafe fn force_unlock() {
        KEY_LOCK.force_unlock();
    }

    pub unsafe fn thread_done() {
        let k = KEY.load(Ordering::Acquire);
        if k == KEY_UNSET {
            return;
        }
        let key = k as libc::pthread_key_t;
        let h = libc::pthread_getspecific(key) as *mut ThreadHeap;
        libc::pthread_setspecific(key, ptr::null());
        let dk = DEFAULT_KEY.load(Ordering::Acquire);
        if dk != KEY_UNSET {
            let dkey = dk as libc::pthread_key_t;
            let d = libc::pthread_getspecific(dkey) as *mut ThreadHeap;
            if d == h {
                libc::pthread_setspecific(dkey, ptr::null());
            }
        }
        heap::abandon(h);
    }

    unsafe extern "C" fn atfork_prepare() {}
    unsafe extern "C" fn atfork_parent() {}
    unsafe extern "C" fn atfork_child() {
        crate::fork_child();
    }

    pub fn register_atfork() {
        unsafe {
            libc::pthread_atfork(
                Some(atfork_prepare),
                Some(atfork_parent),
                Some(atfork_child),
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use crate::heap::ThreadHeap;
    use core::ptr;
    use core::sync::atomic::{AtomicPtr, Ordering};

    static THREAD: AtomicPtr<ThreadHeap> = AtomicPtr::new(ptr::null_mut());
    static DEFAULT: AtomicPtr<ThreadHeap> = AtomicPtr::new(ptr::null_mut());

    pub fn in_recursive_setup() -> bool {
        let me = crate::os::gettid();
        INIT_OWNER.load(core::sync::atomic::Ordering::Acquire) == me
            || (IN_BOOTSTRAP.load(core::sync::atomic::Ordering::Acquire)
                && BOOTSTRAP_TID.load(core::sync::atomic::Ordering::Acquire) == me)
    }

    /// Single process heap (wasm has no threads).
    pub unsafe fn thread_heap() -> *mut ThreadHeap {
        let mut h = THREAD.load(Ordering::Acquire);
        if h.is_null() {
            h = heap::create();
            if h.is_null() {
                return ptr::null_mut();
            }
            THREAD.store(h, Ordering::Release);
        }
        h
    }

    pub unsafe fn default_theap() -> *mut ThreadHeap {
        let t = DEFAULT.load(Ordering::Acquire);
        if !t.is_null() {
            return t;
        }
        thread_heap()
    }

    pub unsafe fn set_default_theap(theap: *mut ThreadHeap) -> *mut ThreadHeap {
        let old = default_theap();
        DEFAULT.store(theap, Ordering::Release);
        old
    }

    #[allow(dead_code)]
    pub unsafe fn force_unlock() {}

    pub unsafe fn thread_done() {
        let h = THREAD.swap(ptr::null_mut(), Ordering::AcqRel);
        DEFAULT.store(ptr::null_mut(), Ordering::Release);
        heap::abandon(h);
    }

    pub fn register_atfork() {}
}

#[cfg(not(target_arch = "wasm32"))]
pub use pthread::*;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;
