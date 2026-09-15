//! Thread-local heap: pthread / `TlsAlloc` keys, a process heap on wasm32.
//!
//! Two keys: the thread's default theap (destructor abandons pages) and
//! the current default used by `malloc` (`mi_heap_set_default`). During
//! key create / first `heap::create`, [`IN_BOOTSTRAP`] / [`CREATE_OWNER`]
//! mark this thread so malloc uses a static bump.

use crate::heap;
use core::sync::atomic::{AtomicBool, AtomicU32};

/// True while creating the TLS key or a thread heap; [`alloc::malloc`](crate::alloc::malloc) uses the bump.
pub static IN_BOOTSTRAP: AtomicBool = AtomicBool::new(false);
/// Thread that is in bootstrap; other threads spin until [`crate::init`] finishes.
pub static BOOTSTRAP_TID: AtomicU32 = AtomicU32::new(0);
/// Thread holding [`crate::init`]'s lock (nested malloc must not take it).
pub static INIT_OWNER: AtomicU32 = AtomicU32::new(0);
/// Thread inside `heap::create` (0 = none). Recursive malloc on this tid uses
/// the bootstrap bump; other threads wait.
pub static CREATE_OWNER: AtomicU32 = AtomicU32::new(0);

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use crate::heap::ThreadHeap;
    use crate::os::TlsSlot;
    use crate::spin::SpinLock;
    use core::ffi::c_void;
    use core::ptr::{self, NonNull};
    use core::sync::atomic::Ordering;

    static KEY: TlsSlot = TlsSlot::new();
    static DEFAULT_KEY: TlsSlot = TlsSlot::new();
    static KEY_LOCK: SpinLock = SpinLock::new();

    pub fn in_recursive_setup() -> bool {
        let me = crate::os::thread_id();
        INIT_OWNER.load(Ordering::Acquire) == me
            || (IN_BOOTSTRAP.load(Ordering::Acquire) && BOOTSTRAP_TID.load(Ordering::Acquire) == me)
            || CREATE_OWNER.load(Ordering::Acquire) == me
    }

    fn begin_bootstrap() {
        let me = crate::os::thread_id();
        BOOTSTRAP_TID.store(me, Ordering::Release);
        IN_BOOTSTRAP.store(true, Ordering::Release);
    }

    fn end_bootstrap() {
        IN_BOOTSTRAP.store(false, Ordering::Release);
        BOOTSTRAP_TID.store(0, Ordering::Release);
    }

    fn wait_while_creating() {
        let mut spins = 0u32;
        while CREATE_OWNER.load(Ordering::Acquire) != 0 {
            spins = spins.wrapping_add(1);
            if spins > 100 {
                crate::os::yield_now();
            } else {
                core::hint::spin_loop();
            }
        }
    }

    unsafe extern "C" fn thread_dtor(ptr: *mut c_void) {
        let h = ptr as *mut ThreadHeap;
        if h.is_null() || (h as usize) < 64 {
            return;
        }
        heap::abandon(h);
    }

    fn ensure_key() {
        if KEY.is_ready() {
            return;
        }
        let _g = KEY_LOCK.lock();
        if KEY.is_ready() {
            return;
        }
        begin_bootstrap();
        KEY.ensure(Some(thread_dtor));
        end_bootstrap();
    }

    fn ensure_default_key() {
        if DEFAULT_KEY.is_ready() {
            return;
        }
        let _g = KEY_LOCK.lock();
        if DEFAULT_KEY.is_ready() {
            return;
        }
        begin_bootstrap();
        DEFAULT_KEY.ensure(None);
        end_bootstrap();
    }

    pub unsafe fn thread_heap() -> *mut ThreadHeap {
        let me = crate::os::thread_id();
        if CREATE_OWNER.load(Ordering::Acquire) == me {
            return ptr::null_mut();
        }
        ensure_key();
        if let Some(h) = KEY.get_non_null::<ThreadHeap>() {
            return h.as_ptr();
        }
        loop {
            if CREATE_OWNER.load(Ordering::Acquire) == me {
                return ptr::null_mut();
            }
            if let Some(h) = KEY.get_non_null::<ThreadHeap>() {
                return h.as_ptr();
            }
            match CREATE_OWNER.compare_exchange(0, me, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(_) => wait_while_creating(),
            }
        }
        struct CreateGuard;
        impl Drop for CreateGuard {
            fn drop(&mut self) {
                CREATE_OWNER.store(0, Ordering::Release);
            }
        }
        let _g = CreateGuard;
        let h = heap::create();
        if !h.is_null() {
            KEY.set_non_null(NonNull::new(h));
        }
        h
    }

    pub unsafe fn default_theap() -> *mut ThreadHeap {
        if DEFAULT_KEY.is_ready() {
            if let Some(t) = DEFAULT_KEY.get_non_null::<ThreadHeap>() {
                return t.as_ptr();
            }
        } else {
            ensure_default_key();
            if let Some(t) = DEFAULT_KEY.get_non_null::<ThreadHeap>() {
                return t.as_ptr();
            }
        }
        thread_heap()
    }

    pub unsafe fn set_default_theap(theap: *mut ThreadHeap) -> *mut ThreadHeap {
        let old = default_theap();
        ensure_default_key();
        DEFAULT_KEY.set_non_null(NonNull::new(theap));
        old
    }

    #[allow(dead_code)]
    pub unsafe fn force_unlock() {
        KEY_LOCK.force_unlock();
        CREATE_OWNER.store(0, Ordering::Release);
    }

    pub unsafe fn thread_done() {
        if !KEY.is_ready() {
            return;
        }
        let h = KEY
            .get_non_null::<ThreadHeap>()
            .map(|n| n.as_ptr())
            .unwrap_or(ptr::null_mut());
        KEY.set_non_null::<ThreadHeap>(None);
        if DEFAULT_KEY.is_ready() {
            let d = DEFAULT_KEY
                .get_non_null::<ThreadHeap>()
                .map(|n| n.as_ptr())
                .unwrap_or(ptr::null_mut());
            if d == h {
                DEFAULT_KEY.set_non_null::<ThreadHeap>(None);
            }
        }
        heap::abandon(h);
    }

    #[cfg(unix)]
    pub fn register_atfork() {
        unsafe extern "C" fn atfork_prepare() {}
        unsafe extern "C" fn atfork_parent() {}
        unsafe extern "C" fn atfork_child() {
            crate::fork_child();
        }
        unsafe {
            libc::pthread_atfork(
                Some(atfork_prepare),
                Some(atfork_parent),
                Some(atfork_child),
            );
        }
    }

    #[cfg(not(unix))]
    pub fn register_atfork() {}
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
        let me = crate::os::thread_id();
        INIT_OWNER.load(core::sync::atomic::Ordering::Acquire) == me
            || (IN_BOOTSTRAP.load(core::sync::atomic::Ordering::Acquire)
                && BOOTSTRAP_TID.load(core::sync::atomic::Ordering::Acquire) == me)
            || CREATE_OWNER.load(core::sync::atomic::Ordering::Acquire) == me
    }

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
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;
