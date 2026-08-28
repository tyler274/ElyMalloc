//! User callbacks: deferred free (to reclaim memory) and error reporting.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

type DeferredFun = unsafe extern "C" fn(bool, u64, *mut c_void);
type ErrorFun = unsafe extern "C" fn(i32, *mut c_void);

static DEFERRED_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static DEFERRED_ARG: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static ERR_FN: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static ERR_ARG: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static DEFERRED_RECURSE: AtomicBool = AtomicBool::new(false);
static ERR_RECURSE: AtomicBool = AtomicBool::new(false);

pub fn register_deferred_free(f: *mut c_void, arg: *mut c_void) {
    DEFERRED_ARG.store(arg, Ordering::Release);
    DEFERRED_FN.store(f as *mut (), Ordering::Release);
}

pub fn register_error(f: *mut c_void, arg: *mut c_void) {
    ERR_ARG.store(arg, Ordering::Release);
    ERR_FN.store(f as *mut (), Ordering::Release);
}

/// Call the registered deferred-free function, if any. Re-entrancy is skipped.
pub fn deferred_free(force: bool, heartbeat: u64) {
    let f = DEFERRED_FN.load(Ordering::Acquire);
    if f.is_null() {
        return;
    }
    if DEFERRED_RECURSE.swap(true, Ordering::AcqRel) {
        return;
    }
    let arg = DEFERRED_ARG.load(Ordering::Acquire);
    unsafe {
        let cb: DeferredFun = core::mem::transmute(f);
        cb(force, heartbeat, arg);
    }
    DEFERRED_RECURSE.store(false, Ordering::Release);
}

/// Invoke the registered error handler. `errno` is already set by the caller.
pub fn error(err: i32) {
    let f = ERR_FN.load(Ordering::Acquire);
    if f.is_null() {
        return;
    }
    if ERR_RECURSE.swap(true, Ordering::AcqRel) {
        return;
    }
    let arg = ERR_ARG.load(Ordering::Acquire);
    unsafe {
        let cb: ErrorFun = core::mem::transmute(f);
        cb(err, arg);
    }
    ERR_RECURSE.store(false, Ordering::Release);
}
