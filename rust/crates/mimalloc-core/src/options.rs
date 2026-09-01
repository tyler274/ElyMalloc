//! `mi_option_t` numeric values matching C `include/mimalloc.h`.
//!
//! Options are process-wide atomics. Unset values can be overridden from
//! `MIMALLOC_*` environment variables at [`init`]. Guards (`guarded_min` /
//! `guarded_max` / sample rate) are honored; many OS-commit options are
//! accepted for ABI compatibility but do not change this rewrite's always-on
//! mitigations.

use core::sync::atomic::{AtomicBool, AtomicI64, Ordering};

pub const OPTION_COUNT: usize = 48;

/// Names matching `mi_option_e` in `include/mimalloc.h` (index 0 .. `_mi_option_last-1`).
pub const NAMES: [&str; 47] = [
    "show_errors",
    "show_stats",
    "verbose",
    "deprecated_eager_commit",
    "arena_eager_commit",
    "purge_decommits",
    "allow_large_os_pages",
    "reserve_huge_os_pages",
    "reserve_huge_os_pages_at",
    "reserve_os_memory",
    "deprecated_segment_cache",
    "deprecated_page_reset",
    "deprecated_abandoned_page_purge",
    "deprecated_segment_reset",
    "deprecated_eager_commit_delay",
    "purge_delay",
    "use_numa_nodes",
    "disallow_os_alloc",
    "os_tag",
    "max_errors",
    "max_warnings",
    "deprecated_max_segment_reclaim",
    "destroy_on_exit",
    "arena_reserve",
    "arena_purge_mult",
    "deprecated_purge_extend_delay",
    "disallow_arena_alloc",
    "retry_on_oom",
    "deprecated_visit_abandoned",
    "guarded_min",
    "guarded_max",
    "guarded_precise",
    "guarded_sample_rate",
    "guarded_sample_seed",
    "generic_collect",
    "page_reclaim_on_free",
    "page_full_retain",
    "page_max_candidates",
    "max_vabits",
    "pagemap_commit",
    "page_commit_on_demand",
    "page_max_reclaim",
    "page_cross_thread_max_reclaim",
    "allow_thp",
    "minimal_purge_size",
    "arena_max_object_size",
    "arena_is_numa_local",
];

static VALUES: [AtomicI64; OPTION_COUNT] = unsafe { core::mem::zeroed() };
static INIT_DONE: AtomicBool = AtomicBool::new(false);

/// Load `MIMALLOC_*` from the environment once.
pub fn init() {
    if INIT_DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    set(5, 1); // purge_decommits
    set(15, 10); // purge_delay
    set(18, 100); // os_tag
    set(32, 0); // guarded_sample_rate (C default 0 unless MI_GUARDED)
    set(34, 10000); // generic_collect
    set(36, 2); // page_full_retain
    set(37, 4); // page_max_candidates
    set(42, 16); // page_cross_thread_max_reclaim
    set(43, 1); // allow_thp
    apply_env();
}

fn apply_env() {
    #[cfg(not(target_arch = "wasm32"))]
    apply_env_linux();
}

#[cfg(not(target_arch = "wasm32"))]
fn apply_env_linux() {
    for (i, name) in NAMES.iter().enumerate() {
        let mut key = [0u8; 80];
        let prefix = b"mimalloc_";
        if prefix.len() + name.len() + 1 > key.len() {
            continue;
        }
        key[..prefix.len()].copy_from_slice(prefix);
        key[prefix.len()..prefix.len() + name.len()].copy_from_slice(name.as_bytes());
        unsafe {
            let v = libc::getenv(key.as_ptr() as *const libc::c_char);
            if v.is_null() {
                continue;
            }
            if let Some(n) = parse_env(v) {
                set(i as i32, n);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn parse_env(s: *const libc::c_char) -> Option<i64> {
    let n = libc::strlen(s);
    if n == 0 {
        return Some(1);
    }
    let bytes = core::slice::from_raw_parts(s as *const u8, n);
    let mut tmp = [0u8; 64];
    let len = n.min(tmp.len());
    for i in 0..len {
        tmp[i] = bytes[i].to_ascii_uppercase();
    }
    let u = &tmp[..len];
    if u == b"1" || u == b"TRUE" || u == b"YES" || u == b"ON" {
        return Some(1);
    }
    if u == b"0" || u == b"FALSE" || u == b"NO" || u == b"OFF" {
        return Some(0);
    }
    let mut val: i64 = 0;
    let mut i = 0;
    let mut neg = false;
    if u[0] == b'-' {
        neg = true;
        i = 1;
    }
    if i >= len {
        return None;
    }
    while i < len && u[i].is_ascii_digit() {
        val = val.saturating_mul(10).saturating_add((u[i] - b'0') as i64);
        i += 1;
    }
    if i != len {
        return None;
    }
    Some(if neg { -val } else { val })
}

/// `mi_option_name`.
pub fn name(option: i32) -> Option<&'static str> {
    NAMES.get(option as usize).copied()
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

/// `mi_option_get` / `mi_option_set`. Unknown indices are 0 / ignored.
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
