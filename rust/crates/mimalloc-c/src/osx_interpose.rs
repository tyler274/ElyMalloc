//! `DYLD_INSERT_LIBRARIES` interpose + a default malloc zone (C `alloc-override.c`).
//!
//! Darwin `fork` walks every registered zone and calls `introspect->force_lock`
//! / `force_unlock` / `reinit_lock` with no NULL check. A null introspect table
//! is a jump to address 0 in the child (and often the parent) — C mimalloc's
//! `intro_reinit_lock` comment in `src/prim/osx/alloc-override-zone.c`.
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
    fn _malloc_fork_prepare();
    fn _malloc_fork_parent();
    fn _malloc_fork_child();
}

unsafe extern "C" fn mi_malloc_fork_prepare() {}
unsafe extern "C" fn mi_malloc_fork_parent() {}
unsafe extern "C" fn mi_malloc_fork_child() {}

#[used]
#[link_section = "__DATA,__interpose"]
static INTERPOSE: [Interpose; 11] = [
    pair!(malloc, libc::malloc),
    pair!(calloc, libc::calloc),
    pair!(realloc, libc::realloc),
    pair!(free, libc::free),
    pair!(posix_memalign, libc::posix_memalign),
    pair!(valloc, dyld_valloc),
    pair!(aligned_alloc, dyld_aligned_alloc),
    pair!(malloc_size_interpose, dyld_malloc_size),
    pair!(mi_malloc_fork_prepare, _malloc_fork_prepare),
    pair!(mi_malloc_fork_parent, _malloc_fork_parent),
    pair!(mi_malloc_fork_child, _malloc_fork_child),
];

unsafe extern "C" fn malloc_size_interpose(p: *mut c_void) -> usize {
    mi_usable_size(p as *const c_void)
}

/// `malloc_introspection_t` through `reinit_lock` (zone version >= 9).
#[repr(C)]
struct MallocIntrospect {
    enumerator:
        Option<unsafe extern "C" fn(u32, *mut c_void, u32, usize, *mut c_void, *mut c_void) -> i32>,
    good_size: Option<unsafe extern "C" fn(*mut MallocZone, usize) -> usize>,
    check: Option<unsafe extern "C" fn(*mut MallocZone) -> u32>,
    print: Option<unsafe extern "C" fn(*mut MallocZone, u32)>,
    log: Option<unsafe extern "C" fn(*mut MallocZone, *mut c_void)>,
    force_lock: Option<unsafe extern "C" fn(*mut MallocZone)>,
    force_unlock: Option<unsafe extern "C" fn(*mut MallocZone)>,
    statistics: Option<unsafe extern "C" fn(*mut MallocZone, *mut MallocStatistics)>,
    zone_locked: Option<unsafe extern "C" fn(*mut MallocZone) -> u32>,
    enable_discharge_checking: Option<unsafe extern "C" fn(*mut MallocZone) -> u32>,
    disable_discharge_checking: Option<unsafe extern "C" fn(*mut MallocZone) -> u32>,
    discharge: Option<unsafe extern "C" fn(*mut MallocZone, *mut c_void)>,
    enumerate_discharged_pointers: Option<unsafe extern "C" fn(*mut MallocZone, *mut c_void)>,
    reinit_lock: Option<unsafe extern "C" fn(*mut MallocZone)>,
}

#[repr(C)]
struct MallocStatistics {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}

/// `malloc_zone_t` version 10 (`claimed_address`).
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
    batch_malloc:
        Option<unsafe extern "C" fn(*mut MallocZone, usize, *mut *mut c_void, u32) -> u32>,
    batch_free: Option<unsafe extern "C" fn(*mut MallocZone, *mut *mut c_void, u32)>,
    introspect: *mut MallocIntrospect,
    version: u32,
    memalign: Option<unsafe extern "C" fn(*mut MallocZone, usize, usize) -> *mut c_void>,
    free_definite_size: Option<unsafe extern "C" fn(*mut MallocZone, *mut c_void, usize)>,
    pressure_relief: Option<unsafe extern "C" fn(*mut MallocZone, usize) -> usize>,
    claimed_address: Option<unsafe extern "C" fn(*mut MallocZone, *mut c_void) -> u32>,
}

unsafe impl Sync for MallocZone {}
unsafe impl Sync for MallocIntrospect {}

unsafe extern "C" fn zone_size(_: *mut MallocZone, p: *const c_void) -> usize {
    if mi_is_in_heap_region(p) {
        mi_usable_size(p)
    } else {
        0
    }
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
unsafe extern "C" fn zone_batch_malloc(
    zone: *mut MallocZone,
    size: usize,
    results: *mut *mut c_void,
    count: u32,
) -> u32 {
    let mut i = 0u32;
    while i < count {
        let p = zone_malloc(zone, size);
        *results.add(i as usize) = p;
        if p.is_null() {
            break;
        }
        i += 1;
    }
    i
}
unsafe extern "C" fn zone_batch_free(zone: *mut MallocZone, ps: *mut *mut c_void, count: u32) {
    for i in 0..count as usize {
        zone_free(zone, *ps.add(i));
        *ps.add(i) = ptr::null_mut();
    }
}
unsafe extern "C" fn zone_pressure_relief(_: *mut MallocZone, _: usize) -> usize {
    mi_collect(false);
    0
}
unsafe extern "C" fn zone_claimed_address(_: *mut MallocZone, p: *mut c_void) -> u32 {
    mi_is_in_heap_region(p as *const c_void) as u32
}

unsafe extern "C" fn intro_enumerator(
    _: u32,
    _: *mut c_void,
    _: u32,
    _: usize,
    _: *mut c_void,
    _: *mut c_void,
) -> i32 {
    0
}
unsafe extern "C" fn intro_good_size(_: *mut MallocZone, size: usize) -> usize {
    mi_good_size(size)
}
unsafe extern "C" fn intro_check(_: *mut MallocZone) -> u32 {
    1
}
unsafe extern "C" fn intro_print(_: *mut MallocZone, _: u32) {
    mi_stats_print(ptr::null_mut());
}
unsafe extern "C" fn intro_log(_: *mut MallocZone, _: *mut c_void) {}
unsafe extern "C" fn intro_force_lock(_: *mut MallocZone) {}
unsafe extern "C" fn intro_force_unlock(_: *mut MallocZone) {}
unsafe extern "C" fn intro_statistics(_: *mut MallocZone, stats: *mut MallocStatistics) {
    if !stats.is_null() {
        ptr::write_bytes(stats, 0, 1);
    }
}
unsafe extern "C" fn intro_zone_locked(_: *mut MallocZone) -> u32 {
    0
}
unsafe extern "C" fn intro_reinit_lock(_: *mut MallocZone) {}

static INTROSPECT: MallocIntrospect = MallocIntrospect {
    enumerator: Some(intro_enumerator),
    good_size: Some(intro_good_size),
    check: Some(intro_check),
    print: Some(intro_print),
    log: Some(intro_log),
    force_lock: Some(intro_force_lock),
    force_unlock: Some(intro_force_unlock),
    statistics: Some(intro_statistics),
    zone_locked: Some(intro_zone_locked),
    enable_discharge_checking: None,
    disable_discharge_checking: None,
    discharge: None,
    enumerate_discharged_pointers: None,
    reinit_lock: Some(intro_reinit_lock),
};

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
    batch_malloc: Some(zone_batch_malloc),
    batch_free: Some(zone_batch_free),
    introspect: ptr::null_mut(),
    version: 10,
    memalign: Some(zone_memalign),
    free_definite_size: Some(zone_free_sz),
    pressure_relief: Some(zone_pressure_relief),
    claimed_address: Some(zone_claimed_address),
};

unsafe extern "C" {
    fn malloc_zone_register(zone: *mut MallocZone);
}

#[used]
#[link_section = "__DATA,__mod_init_func"]
static ZONE_INIT: extern "C" fn() = register_zone;

extern "C" fn register_zone() {
    mimalloc_core::init();
    unsafe {
        ZONE.introspect = core::ptr::addr_of!(INTROSPECT) as *mut MallocIntrospect;
        malloc_zone_register(core::ptr::addr_of_mut!(ZONE));
    }
}
