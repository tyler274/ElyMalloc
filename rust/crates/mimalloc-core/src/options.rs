//! `mi_option_t` numeric values matching C `include/mimalloc.h`.

use core::sync::atomic::{AtomicBool, AtomicI64, Ordering};

pub const OPTION_COUNT: usize = 48;

static VALUES: [AtomicI64; OPTION_COUNT] = unsafe { core::mem::zeroed() };
static INIT_DONE: AtomicBool = AtomicBool::new(false);

pub fn init() {
    if INIT_DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    set(5, 1); // purge_decommits
    set(15, 10); // purge_delay
    set(18, 100); // os_tag
    set(32, 1000); // guarded_sample_rate
    set(34, 10000); // generic_collect
    set(36, 2); // page_full_retain
    set(37, 4); // page_max_candidates
    set(42, 16); // page_cross_thread_max_reclaim
    set(43, 1); // allow_thp
}

#[inline]
fn slot(option: i32) -> Option<&'static AtomicI64> {
    let i = option as usize;
    if i < OPTION_COUNT {
        Some(&VALUES[i])
    } else {
        None
    }
}

pub fn get(option: i32) -> i64 {
    init();
    slot(option).map(|s| s.load(Ordering::Relaxed)).unwrap_or(0)
}

pub fn set(option: i32, value: i64) {
    if let Some(s) = slot(option) {
        s.store(value, Ordering::Relaxed);
    }
}

pub fn is_enabled(option: i32) -> bool {
    get(option) != 0
}

pub fn enable(option: i32) {
    set(option, 1);
}

pub fn disable(option: i32) {
    set(option, 0);
}

pub fn get_size(option: i32) -> usize {
    let v = get(option);
    if v <= 0 {
        0
    } else {
        (v as usize).saturating_mul(1024)
    }
}

pub fn clamp(option: i32, min: i64, max: i64) -> i64 {
    get(option).clamp(min, max)
}
