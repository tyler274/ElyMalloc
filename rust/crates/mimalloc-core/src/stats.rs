//! Process-wide counters used to fill `mi_stats_t`.

use core::sync::atomic::{AtomicI64, Ordering};

static PAGES_CURRENT: AtomicI64 = AtomicI64::new(0);
static PAGES_TOTAL: AtomicI64 = AtomicI64::new(0);
static PAGES_PEAK: AtomicI64 = AtomicI64::new(0);
static HEAPS_CURRENT: AtomicI64 = AtomicI64::new(0);
static MALLOC_CURRENT: AtomicI64 = AtomicI64::new(0);
static MALLOC_TOTAL: AtomicI64 = AtomicI64::new(0);
static MALLOC_PEAK: AtomicI64 = AtomicI64::new(0);
static MALLOC_COUNT: AtomicI64 = AtomicI64::new(0);
static RESERVED_CURRENT: AtomicI64 = AtomicI64::new(0);
static RESERVED_TOTAL: AtomicI64 = AtomicI64::new(0);
static RESERVED_PEAK: AtomicI64 = AtomicI64::new(0);
static COMMITTED_CURRENT: AtomicI64 = AtomicI64::new(0);
static COMMITTED_TOTAL: AtomicI64 = AtomicI64::new(0);
static COMMITTED_PEAK: AtomicI64 = AtomicI64::new(0);
static MMAP_CALLS: AtomicI64 = AtomicI64::new(0);
static PURGED: AtomicI64 = AtomicI64::new(0);
static PURGE_CALLS: AtomicI64 = AtomicI64::new(0);
static ARENA_COUNT: AtomicI64 = AtomicI64::new(0);

fn peak_add(current: &AtomicI64, peak: &AtomicI64, total: Option<&AtomicI64>, n: i64) {
    if let Some(total) = total {
        total.fetch_add(n, Ordering::Relaxed);
    }
    let cur = current.fetch_add(n, Ordering::Relaxed) + n;
    loop {
        let p = peak.load(Ordering::Relaxed);
        if cur <= p {
            break;
        }
        if peak
            .compare_exchange_weak(p, cur, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

pub fn page_add() {
    peak_add(&PAGES_CURRENT, &PAGES_PEAK, Some(&PAGES_TOTAL), 1);
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
    (*out).malloc_requested.current = MALLOC_CURRENT.load(Ordering::Relaxed);
    (*out).malloc_requested.total = MALLOC_TOTAL.load(Ordering::Relaxed);
    (*out).malloc_requested.peak = MALLOC_PEAK.load(Ordering::Relaxed);
    (*out).malloc_normal.current = MALLOC_CURRENT.load(Ordering::Relaxed);
    (*out).malloc_normal.total = MALLOC_TOTAL.load(Ordering::Relaxed);
    (*out).malloc_normal.peak = MALLOC_PEAK.load(Ordering::Relaxed);
    (*out).malloc_normal_count.total = MALLOC_COUNT.load(Ordering::Relaxed);
    (*out).reserved.current = RESERVED_CURRENT.load(Ordering::Relaxed);
    (*out).reserved.total = RESERVED_TOTAL.load(Ordering::Relaxed);
    (*out).reserved.peak = RESERVED_PEAK.load(Ordering::Relaxed);
    (*out).committed.current = COMMITTED_CURRENT.load(Ordering::Relaxed);
    (*out).committed.total = COMMITTED_TOTAL.load(Ordering::Relaxed);
    (*out).committed.peak = COMMITTED_PEAK.load(Ordering::Relaxed);
    (*out).mmap_calls.total = MMAP_CALLS.load(Ordering::Relaxed);
    (*out).purged.total = PURGED.load(Ordering::Relaxed);
    (*out).purge_calls.total = PURGE_CALLS.load(Ordering::Relaxed);
    (*out).arena_count.total = ARENA_COUNT.load(Ordering::Relaxed);
    (*out).malloc_guarded_count.total = GUARDED_COUNT.load(Ordering::Relaxed);
}

pub unsafe fn clear(out: *mut Stats) {
    ptr_zero(out);
    if !out.is_null() {
        (*out).size = core::mem::size_of::<Stats>();
        (*out).version = STAT_VERSION;
    }
}

unsafe fn ptr_zero<T>(p: *mut T) {
    core::ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<T>());
}

/// Compact counters stored on each thread-heap / subprocess.
#[repr(C)]
pub struct AllocStats {
    pub malloc_current: AtomicI64,
    pub malloc_total: AtomicI64,
    pub malloc_peak: AtomicI64,
    pub malloc_count: AtomicI64,
    pub pages_current: AtomicI64,
    pub pages_total: AtomicI64,
    pub pages_peak: AtomicI64,
}

impl AllocStats {
    pub fn add_malloc(&self, bytes: usize) {
        peak_add(
            &self.malloc_current,
            &self.malloc_peak,
            Some(&self.malloc_total),
            bytes as i64,
        );
        self.malloc_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn sub_malloc(&self, bytes: usize) {
        self.malloc_current
            .fetch_sub(bytes as i64, Ordering::Relaxed);
    }

    pub fn add_page(&self) {
        peak_add(
            &self.pages_current,
            &self.pages_peak,
            Some(&self.pages_total),
            1,
        );
    }

    pub fn sub_page(&self) {
        self.pages_current.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn merge_from(&self, other: &AllocStats) {
        let n = other.malloc_current.load(Ordering::Relaxed);
        if n != 0 {
            peak_add(
                &self.malloc_current,
                &self.malloc_peak,
                Some(&self.malloc_total),
                n,
            );
        }
        self.malloc_count.fetch_add(
            other.malloc_count.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        let p = other.pages_current.load(Ordering::Relaxed);
        if p != 0 {
            peak_add(
                &self.pages_current,
                &self.pages_peak,
                Some(&self.pages_total),
                p,
            );
        }
    }

    pub unsafe fn copy_into(&self, out: *mut Stats) {
        if out.is_null() {
            return;
        }
        (*out).malloc_requested.current = self.malloc_current.load(Ordering::Relaxed);
        (*out).malloc_requested.total = self.malloc_total.load(Ordering::Relaxed);
        (*out).malloc_requested.peak = self.malloc_peak.load(Ordering::Relaxed);
        (*out).malloc_normal.current = (*out).malloc_requested.current;
        (*out).malloc_normal.total = (*out).malloc_requested.total;
        (*out).malloc_normal.peak = (*out).malloc_requested.peak;
        (*out).malloc_normal_count.total = self.malloc_count.load(Ordering::Relaxed);
        (*out).pages.current = self.pages_current.load(Ordering::Relaxed);
        (*out).pages.total = self.pages_total.load(Ordering::Relaxed);
        (*out).pages.peak = self.pages_peak.load(Ordering::Relaxed);
    }

    pub unsafe fn add_into(&self, out: *mut Stats) {
        if out.is_null() {
            return;
        }
        (*out).malloc_requested.current += self.malloc_current.load(Ordering::Relaxed);
        (*out).malloc_requested.total += self.malloc_total.load(Ordering::Relaxed);
        if self.malloc_peak.load(Ordering::Relaxed) > (*out).malloc_requested.peak {
            (*out).malloc_requested.peak = self.malloc_peak.load(Ordering::Relaxed);
        }
        (*out).malloc_normal.current = (*out).malloc_requested.current;
        (*out).malloc_normal.total = (*out).malloc_requested.total;
        (*out).malloc_normal.peak = (*out).malloc_requested.peak;
        (*out).malloc_normal_count.total += self.malloc_count.load(Ordering::Relaxed);
        (*out).pages.current += self.pages_current.load(Ordering::Relaxed);
        (*out).pages.total += self.pages_total.load(Ordering::Relaxed);
        if self.pages_peak.load(Ordering::Relaxed) > (*out).pages.peak {
            (*out).pages.peak = self.pages_peak.load(Ordering::Relaxed);
        }
    }
}

pub fn reset() {
    PAGES_CURRENT.store(0, Ordering::Relaxed);
    PAGES_TOTAL.store(0, Ordering::Relaxed);
    PAGES_PEAK.store(0, Ordering::Relaxed);
    MALLOC_CURRENT.store(0, Ordering::Relaxed);
    MALLOC_TOTAL.store(0, Ordering::Relaxed);
    MALLOC_PEAK.store(0, Ordering::Relaxed);
    MALLOC_COUNT.store(0, Ordering::Relaxed);
    RESERVED_CURRENT.store(0, Ordering::Relaxed);
    RESERVED_TOTAL.store(0, Ordering::Relaxed);
    RESERVED_PEAK.store(0, Ordering::Relaxed);
    COMMITTED_CURRENT.store(0, Ordering::Relaxed);
    COMMITTED_TOTAL.store(0, Ordering::Relaxed);
    COMMITTED_PEAK.store(0, Ordering::Relaxed);
    MMAP_CALLS.store(0, Ordering::Relaxed);
    PURGED.store(0, Ordering::Relaxed);
    PURGE_CALLS.store(0, Ordering::Relaxed);
    ARENA_COUNT.store(0, Ordering::Relaxed);
    GUARDED_COUNT.store(0, Ordering::Relaxed);
}

pub fn get_bin_size(bin: usize) -> usize {
    crate::bin::bin_size(bin)
}

pub fn malloc_add(bytes: usize) {
    peak_add(
        &MALLOC_CURRENT,
        &MALLOC_PEAK,
        Some(&MALLOC_TOTAL),
        bytes as i64,
    );
    MALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
}

static GUARDED_COUNT: AtomicI64 = AtomicI64::new(0);

pub fn malloc_guarded_add() {
    GUARDED_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn malloc_sub(bytes: usize) {
    MALLOC_CURRENT.fetch_sub(bytes as i64, Ordering::Relaxed);
}

pub fn mmap_map(size: usize, committed: bool) {
    let n = size as i64;
    MMAP_CALLS.fetch_add(1, Ordering::Relaxed);
    peak_add(&RESERVED_CURRENT, &RESERVED_PEAK, Some(&RESERVED_TOTAL), n);
    if committed {
        peak_add(
            &COMMITTED_CURRENT,
            &COMMITTED_PEAK,
            Some(&COMMITTED_TOTAL),
            n,
        );
    }
}

pub fn mmap_unmap(size: usize, committed: bool) {
    let n = size as i64;
    RESERVED_CURRENT.fetch_sub(n, Ordering::Relaxed);
    if committed {
        COMMITTED_CURRENT.fetch_sub(n, Ordering::Relaxed);
    }
}

pub fn commit_add(size: usize) {
    peak_add(
        &COMMITTED_CURRENT,
        &COMMITTED_PEAK,
        Some(&COMMITTED_TOTAL),
        size as i64,
    );
}

pub fn purge(size: usize) {
    PURGE_CALLS.fetch_add(1, Ordering::Relaxed);
    PURGED.fetch_add(size as i64, Ordering::Relaxed);
}

pub fn arena_add() {
    ARENA_COUNT.fetch_add(1, Ordering::Relaxed);
}
