//! `DYLD_INSERT_LIBRARIES` interpose + a default malloc zone (C `alloc-override.c`).
#![allow(non_camel_case_types)]

use super::*;
use core::ptr;

#[repr(C)]
struct Interpose {
    replacement: *const c_void,
    target: *const c_void,
}

unsafe impl Sync for Interpose {}

macro_rules! pair {
    ($new:expr, $old:expr) => {
        Interpose {
            replacement: $new as *const c_void,
            target: $old as *const c_void,
        }
    };
}

unsafe extern "C" {
    #[link_name = "valloc"]
    fn dyld_valloc(size: usize) -> *mut c_void;
    #[link_name = "aligned_alloc"]
    fn dyld_aligned_alloc(alignment: usize, size: usize) -> *mut c_void;
    #[link_name = "malloc_size"]
    fn dyld_malloc_size(p: *mut c_void) -> usize;
}

#[used]
#[link_section = "__DATA,__interpose"]
static INTERPOSE: [Interpose; 8] = [
    pair!(malloc, libc::malloc),
    pair!(calloc, libc::calloc),
    pair!(realloc, libc::realloc),
    pair!(free, libc::free),
    pair!(posix_memalign, libc::posix_memalign),
    pair!(valloc, dyld_valloc),
    pair!(aligned_alloc, dyld_aligned_alloc),
    pair!(malloc_size_interpose, dyld_malloc_size),
];

unsafe extern "C" fn malloc_size_interpose(p: *mut c_void) -> usize {
    mi_usable_size(p as *const c_void)
}

/// Minimal `malloc_zone_t` (version 0 callbacks). Extra fields stay zero.
#[repr(C)]
struct MallocZone {
    reserved1: *mut c_void,
    reserved2: *mut c_void,
    size: Option<unsafe extern "C" fn(*mut MallocZone, *const c_void) -> usize>,
    malloc: Option<unsafe extern "C" fn(*mut MallocZone, usize) -> *mut c_void>,
    calloc: Option<unsafe extern "C" fn(*mut MallocZone, usize, usize) -> *mut c_void>,
    valloc: Option<unsafe extern "C" fn(*mut MallocZone, usize) -> *mut c_void>,
    free: Option<unsafe extern "C" fn(*mut MallocZone, *mut c_void)>,
    realloc: Option<unsafe extern "C" fn(*mut MallocZone, *mut c_void, usize) -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut MallocZone)>,
    zone_name: *const u8,
    batch_malloc: *mut c_void,
    batch_free: *mut c_void,
    introspect: *mut c_void,
    version: u32,
    memalign: Option<unsafe extern "C" fn(*mut MallocZone, usize, usize) -> *mut c_void>,
    free_definite_size: Option<unsafe extern "C" fn(*mut MallocZone, *mut c_void, usize)>,
}

unsafe extern "C" fn zone_size(_: *mut MallocZone, p: *const c_void) -> usize {
    mi_usable_size(p)
}
unsafe extern "C" fn zone_malloc(_: *mut MallocZone, n: usize) -> *mut c_void {
    malloc(n)
}
unsafe extern "C" fn zone_calloc(_: *mut MallocZone, c: usize, n: usize) -> *mut c_void {
    calloc(c, n)
}
unsafe extern "C" fn zone_valloc(_: *mut MallocZone, n: usize) -> *mut c_void {
    valloc(n)
}
unsafe extern "C" fn zone_free(_: *mut MallocZone, p: *mut c_void) {
    free(p);
}
unsafe extern "C" fn zone_realloc(_: *mut MallocZone, p: *mut c_void, n: usize) -> *mut c_void {
    realloc(p, n)
}
unsafe extern "C" fn zone_destroy(_: *mut MallocZone) {}
unsafe extern "C" fn zone_memalign(_: *mut MallocZone, a: usize, n: usize) -> *mut c_void {
    memalign(a, n)
}
unsafe extern "C" fn zone_free_sz(_: *mut MallocZone, p: *mut c_void, _: usize) {
    free(p);
}

static mut ZONE: MallocZone = MallocZone {
    reserved1: ptr::null_mut(),
    reserved2: ptr::null_mut(),
    size: Some(zone_size),
    malloc: Some(zone_malloc),
    calloc: Some(zone_calloc),
    valloc: Some(zone_valloc),
    free: Some(zone_free),
    realloc: Some(zone_realloc),
    destroy: Some(zone_destroy),
    zone_name: b"mimalloc\0".as_ptr(),
    batch_malloc: ptr::null_mut(),
    batch_free: ptr::null_mut(),
    introspect: ptr::null_mut(),
    version: 8,
    memalign: Some(zone_memalign),
    free_definite_size: Some(zone_free_sz),
};

unsafe extern "C" {
    fn malloc_zone_register(zone: *mut MallocZone);
}

#[used]
#[link_section = "__DATA,__mod_init_func"]
static ZONE_INIT: extern "C" fn() = register_zone;

extern "C" fn register_zone() {
    unsafe {
        malloc_zone_register(core::ptr::addr_of_mut!(ZONE));
    }
}
