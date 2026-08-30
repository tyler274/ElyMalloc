//! `#[global_allocator]` microbench: wall time and user-mode instructions.
//!
//! Same workloads as `rust/tests/bench.c` so GlobalAlloc can be compared with
//! `LD_PRELOAD` of the C ABI (see `mimalloc-harness bench`).

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use mimalloc_core::Mimalloc;
use std::time::Instant;

#[global_allocator]
static ALLOC: Mimalloc = Mimalloc;

fn n_iters() -> u32 {
    std::env::var("BENCH_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
}

fn main() {
    let scale = n_iters();
    let cases: &[(&str, fn(u32))] = &[
        ("malloc-free-16", |s| malloc_free(16, 2_000_000 / s.max(1))),
        ("malloc-free-64", |s| malloc_free(64, 2_000_000 / s.max(1))),
        ("malloc-free-1024", |s| {
            malloc_free(1024, 400_000 / s.max(1))
        }),
        ("malloc-free-65536", |s| {
            malloc_free(65536, 20_000 / s.max(1))
        }),
        ("calloc-64", |s| calloc_free(64, 400_000 / s.max(1))),
        ("realloc-16-4096", |s| {
            realloc_grow(16, 4096, 200_000 / s.max(1))
        }),
    ];
    for (name, f) in cases {
        let (ns, ins) = measure(|| f(scale));
        println!("bench {name} ns={ns} instructions={ins}");
    }
}

fn malloc_free(size: usize, n: u32) {
    let layout = Layout::from_size_align(size.max(1), 8).unwrap();
    unsafe {
        for _ in 0..n {
            let p = ALLOC.alloc(layout);
            if !p.is_null() {
                ptr::write(p, 1u8);
                ALLOC.dealloc(p, layout);
            }
        }
    }
}

fn calloc_free(size: usize, n: u32) {
    let layout = Layout::from_size_align(size.max(1), 8).unwrap();
    unsafe {
        for _ in 0..n {
            let p = ALLOC.alloc_zeroed(layout);
            if !p.is_null() {
                ALLOC.dealloc(p, layout);
            }
        }
    }
}

fn realloc_grow(old: usize, new: usize, n: u32) {
    let a = Layout::from_size_align(old.max(1), 8).unwrap();
    unsafe {
        for _ in 0..n {
            let p = ALLOC.alloc(a);
            if p.is_null() {
                continue;
            }
            ptr::write(p, 1u8);
            let q = ALLOC.realloc(p, a, new);
            if !q.is_null() {
                ALLOC.dealloc(q, Layout::from_size_align(new, 8).unwrap());
            }
        }
    }
}

fn measure(f: impl FnOnce()) -> (u64, u64) {
    let perf = InstrCounter::open();
    if let Some(ref c) = perf {
        c.reset_enable();
    }
    let t0 = Instant::now();
    f();
    let ns = t0.elapsed().as_nanos() as u64;
    let ins = perf.map(|c| c.disable_read()).unwrap_or(0);
    (ns, ins)
}

struct InstrCounter {
    fd: i32,
}

impl InstrCounter {
    fn open() -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            let fd = unsafe { perf_event_open_instructions() };
            if fd >= 0 {
                return Some(Self { fd });
            }
        }
        None
    }

    fn reset_enable(&self) {
        #[cfg(target_os = "linux")]
        unsafe {
            libc::ioctl(self.fd, PERF_EVENT_IOC_RESET, 0);
            libc::ioctl(self.fd, PERF_EVENT_IOC_ENABLE, 0);
        }
    }

    fn disable_read(self) -> u64 {
        #[cfg(target_os = "linux")]
        {
            unsafe {
                libc::ioctl(self.fd, PERF_EVENT_IOC_DISABLE, 0);
            }
            let mut count: u64 = 0;
            let n = unsafe {
                libc::read(
                    self.fd,
                    (&mut count as *mut u64).cast(),
                    core::mem::size_of::<u64>(),
                )
            };
            unsafe {
                libc::close(self.fd);
            }
            if n == 8 {
                return count;
            }
        }
        0
    }
}

// linux/perf_event.h: _IO('$', n) on x86_64/aarch64.
const PERF_EVENT_IOC_ENABLE: libc::c_ulong = 0x2400;
const PERF_EVENT_IOC_DISABLE: libc::c_ulong = 0x2401;
const PERF_EVENT_IOC_RESET: libc::c_ulong = 0x2402;

/// `PERF_ATTR_SIZE_VER0` (64) — enough for hardware instruction counts.
#[repr(C)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    bp_addr: u64,
}

#[cfg(target_os = "linux")]
unsafe fn perf_event_open_instructions() -> i32 {
    let mut pe: PerfEventAttr = core::mem::zeroed();
    pe.type_ = 0; // PERF_TYPE_HARDWARE
    pe.size = core::mem::size_of::<PerfEventAttr>() as u32;
    pe.config = 1; // PERF_COUNT_HW_INSTRUCTIONS
                   // disabled | exclude_kernel | exclude_hv
    pe.flags = 1 | (1 << 5) | (1 << 6);
    libc::syscall(libc::SYS_perf_event_open, &pe, 0, -1isize, -1isize, 0usize) as i32
}
