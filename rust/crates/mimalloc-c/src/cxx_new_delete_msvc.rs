//! MSVC x64 mangled `operator new` / `operator delete`.

use super::*;

#[export_name = "??2@YAPEAX_K@Z"]
pub unsafe extern "C" fn msvc_new(n: usize) -> *mut c_void {
    mi_new(n)
}

#[export_name = "??_U@YAPEAX_K@Z"]
pub unsafe extern "C" fn msvc_new_array(n: usize) -> *mut c_void {
    mi_new(n)
}

#[export_name = "??3@YAXPEAX@Z"]
pub unsafe extern "C" fn msvc_delete(p: *mut c_void) {
    mi_free(p);
}

#[export_name = "??_V@YAXPEAX@Z"]
pub unsafe extern "C" fn msvc_delete_array(p: *mut c_void) {
    mi_free(p);
}

#[export_name = "??3@YAXPEAX_K@Z"]
pub unsafe extern "C" fn msvc_delete_sized(p: *mut c_void, _n: usize) {
    mi_free(p);
}

#[export_name = "??_V@YAXPEAX_K@Z"]
pub unsafe extern "C" fn msvc_delete_array_sized(p: *mut c_void, _n: usize) {
    mi_free(p);
}
