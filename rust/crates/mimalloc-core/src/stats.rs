//! Process-wide counters used to fill `mi_stats_t`.

use core::sync::atomic::{AtomicI64, Ordering};

static PAGES_CURRENT: AtomicI64 = AtomicI64::new(0);
static PAGES_TOTAL: AtomicI64 = AtomicI64::new(0);
static PAGES_PEAK: AtomicI64 = AtomicI64::new(0);
static HEAPS_CURRENT: AtomicI64 = AtomicI64::new(0);

pub fn page_add() {
    let cur = PAGES_CURRENT.fetch_add(1, Ordering::Relaxed) + 1;
    PAGES_TOTAL.fetch_add(1, Ordering::Relaxed);
    loop {
        let peak = PAGES_PEAK.load(Ordering::Relaxed);
        if cur <= peak {
            break;
        }
        if PAGES_PEAK
            .compare_exchange_weak(peak, cur, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

pub fn page_sub() {
    PAGES_CURRENT.fetch_sub(1, Ordering::Relaxed);
}

pub fn heap_add() {
    HEAPS_CURRENT.fetch_add(1, Ordering::Relaxed);
}

pub fn heap_sub() {
    HEAPS_CURRENT.fetch_sub(1, Ordering::Relaxed);
}

pub fn pages_current() -> i64 {
    PAGES_CURRENT.load(Ordering::Relaxed)
}

/// Layout-compatible with C `mi_stat_count_t`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StatCount {
    pub total: i64,
    pub peak: i64,
    pub current: i64,
}

/// Layout-compatible with C `mi_stat_counter_t`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StatCounter {
    pub total: i64,
}

pub const STAT_VERSION: usize = 5;
pub const BIN_HUGE: usize = 73;
pub const CBIN_COUNT: usize = 6;

/// Layout-compatible with C `mi_stats_t` (v3 / `MI_STAT_VERSION` 5).
#[repr(C)]
pub struct Stats {
    pub size: usize,
    pub version: usize,
    pub pages: StatCount,
    pub reserved: StatCount,
    pub committed: StatCount,
    pub reset: StatCounter,
    pub purged: StatCounter,
    pub page_committed: StatCount,
    pub pages_abandoned: StatCount,
    pub threads: StatCount,
    pub malloc_normal: StatCount,
    pub malloc_huge: StatCount,
    pub malloc_requested: StatCount,
    pub mmap_calls: StatCounter,
    pub commit_calls: StatCounter,
    pub reset_calls: StatCounter,
    pub purge_calls: StatCounter,
    pub arena_count: StatCounter,
    pub malloc_normal_count: StatCounter,
    pub malloc_huge_count: StatCounter,
    pub malloc_guarded_count: StatCounter,
    pub arena_rollback_count: StatCounter,
    pub arena_purges: StatCounter,
    pub pages_extended: StatCounter,
    pub pages_retire: StatCounter,
    pub page_searches: StatCounter,
    pub page_searches_count: StatCounter,
    pub segments: StatCount,
    pub segments_abandoned: StatCount,
    pub segments_cache: StatCount,
    pub segments_reserved: StatCount,
    pub heaps: StatCount,
    pub theaps: StatCount,
    pub pages_reclaim_on_alloc: StatCounter,
    pub pages_reclaim_on_free: StatCounter,
    pub pages_reabandon_full: StatCounter,
    pub pages_unabandon_busy_wait: StatCounter,
    pub heaps_delete_wait: StatCounter,
    pub stat_reserved: [StatCount; 4],
    pub stat_counter_reserved: [StatCounter; 4],
    pub malloc_bins: [StatCount; BIN_HUGE + 1],
    pub page_bins: [StatCount; BIN_HUGE + 1],
    pub chunk_bins: [StatCount; CBIN_COUNT],
}

pub unsafe fn fill(out: *mut Stats) {
    if out.is_null() {
        return;
    }
    ptr_zero(out);
    (*out).size = core::mem::size_of::<Stats>();
    (*out).version = STAT_VERSION;
    (*out).pages.current = PAGES_CURRENT.load(Ordering::Relaxed);
    (*out).pages.total = PAGES_TOTAL.load(Ordering::Relaxed);
    (*out).pages.peak = PAGES_PEAK.load(Ordering::Relaxed);
    (*out).heaps.current = HEAPS_CURRENT.load(Ordering::Relaxed);
}

unsafe fn ptr_zero<T>(p: *mut T) {
    core::ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<T>());
}

pub fn reset() {
    PAGES_CURRENT.store(0, Ordering::Relaxed);
    PAGES_TOTAL.store(0, Ordering::Relaxed);
    PAGES_PEAK.store(0, Ordering::Relaxed);
}
