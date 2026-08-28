//! Thread-local heap via pthread keys, with a recursion-safe bootstrap.

use crate::heap::{self, ThreadHeap};
use crate::spin::SpinLock;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

pub static IN_BOOTSTRAP: AtomicBool = AtomicBool::new(false);
pub static BOOTSTRAP_TID: AtomicU32 = AtomicU32::new(0);

const KEY_UNSET: usize = usize::MAX;

static KEY: AtomicUsize = AtomicUsize::new(KEY_UNSET);
static DEFAULT_KEY: AtomicUsize = AtomicUsize::new(KEY_UNSET);
static KEY_LOCK: SpinLock = SpinLock::new();

unsafe extern "C" fn thread_dtor(ptr: *mut libc::c_void) {
    heap::abandon(ptr as *mut ThreadHeap);
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
    unsafe {
        IN_BOOTSTRAP.store(true, Ordering::Release);
        BOOTSTRAP_TID.store(crate::os::gettid(), Ordering::Release);
        let rc = libc::pthread_key_create(&mut key, Some(thread_dtor));
        BOOTSTRAP_TID.store(0, Ordering::Release);
        IN_BOOTSTRAP.store(false, Ordering::Release);
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
    unsafe {
        let rc = libc::pthread_key_create(&mut key, None);
        if rc != 0 {
            crate::os::abort();
        }
    }
    DEFAULT_KEY.store(key as usize, Ordering::Release);
    key
}

#[inline]
pub unsafe fn thread_heap() -> *mut ThreadHeap {
    let key = ensure_key();
    let mut h = libc::pthread_getspecific(key) as *mut ThreadHeap;
    if h.is_null() {
        h = heap::create();
        if h.is_null() {
            return ptr::null_mut();
        }
        libc::pthread_setspecific(key, h as *const libc::c_void);
    }
    h
}

/// Current default theap used by `malloc` (override or this thread's heap).
#[inline]
pub unsafe fn default_theap() -> *mut ThreadHeap {
    let key = ensure_default_key();
    let t = libc::pthread_getspecific(key) as *mut ThreadHeap;
    if !t.is_null() {
        return t;
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

/// Explicit `mi_thread_done`: abandon this thread's heap and clear TLS so the
/// pthread key destructor does not double-free it.
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
