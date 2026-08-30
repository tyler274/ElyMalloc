//! Itanium C++ `new`/`delete`. Omitted from the static archive used to
//! vendor into programs (like mold) that already define these symbols.
//! The shared library keeps them for `LD_PRELOAD` of C++ processes.

use super::*;

#[no_mangle]
pub unsafe extern "C" fn _ZdlPv(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPv(p: *mut c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdlPvm(p: *mut c_void, _n: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPvm(p: *mut c_void, _n: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _Znwm(n: usize) -> *mut c_void {
    mi_new(n)
}

#[no_mangle]
pub unsafe extern "C" fn _Znam(n: usize) -> *mut c_void {
    mi_new(n)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnwmRKSt9nothrow_t(n: usize, _tag: *const c_void) -> *mut c_void {
    mi_new_nothrow(n)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnamRKSt9nothrow_t(n: usize, _tag: *const c_void) -> *mut c_void {
    mi_new_nothrow(n)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnwmSt11align_val_t(n: usize, al: usize) -> *mut c_void {
    mi_new_aligned(n, al)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnamSt11align_val_t(n: usize, al: usize) -> *mut c_void {
    mi_new_aligned(n, al)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnwmSt11align_val_tRKSt9nothrow_t(
    n: usize,
    al: usize,
    _tag: *const c_void,
) -> *mut c_void {
    mi_new_aligned_nothrow(n, al)
}

#[no_mangle]
pub unsafe extern "C" fn _ZnamSt11align_val_tRKSt9nothrow_t(
    n: usize,
    al: usize,
    _tag: *const c_void,
) -> *mut c_void {
    mi_new_aligned_nothrow(n, al)
}

#[no_mangle]
pub unsafe extern "C" fn _ZdlPvSt11align_val_t(p: *mut c_void, _al: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPvSt11align_val_t(p: *mut c_void, _al: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdlPvmSt11align_val_t(p: *mut c_void, _n: usize, _al: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPvmSt11align_val_t(p: *mut c_void, _n: usize, _al: usize) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdlPvRKSt9nothrow_t(p: *mut c_void, _tag: *const c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPvRKSt9nothrow_t(p: *mut c_void, _tag: *const c_void) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdlPvSt11align_val_tRKSt9nothrow_t(
    p: *mut c_void,
    _al: usize,
    _tag: *const c_void,
) {
    mi_free(p);
}

#[no_mangle]
pub unsafe extern "C" fn _ZdaPvSt11align_val_tRKSt9nothrow_t(
    p: *mut c_void,
    _al: usize,
    _tag: *const c_void,
) {
    mi_free(p);
}
