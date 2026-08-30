/* Allocator microbench: wall time + user-mode instructions (perf_event_open).
 * Built by mimalloc-harness and run under LD_PRELOAD of each malloc. */
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#ifdef __linux__
#include <linux/perf_event.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>
#include <unistd.h>
#endif

static uint64_t ns_now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000000000ull + (uint64_t)t.tv_nsec;
}

#ifdef __linux__
static int perf_fd = -1;

static void perf_open(void) {
    struct perf_event_attr pe;
    memset(&pe, 0, sizeof(pe));
    pe.type = PERF_TYPE_HARDWARE;
    pe.size = sizeof(pe);
    pe.config = PERF_COUNT_HW_INSTRUCTIONS;
    pe.disabled = 1;
    pe.exclude_kernel = 1;
    pe.exclude_hv = 1;
    perf_fd = (int)syscall(__NR_perf_event_open, &pe, 0, -1, -1, 0);
}

static void perf_start(void) {
    if (perf_fd < 0) {
        return;
    }
    ioctl(perf_fd, PERF_EVENT_IOC_RESET, 0);
    ioctl(perf_fd, PERF_EVENT_IOC_ENABLE, 0);
}

static uint64_t perf_stop(void) {
    uint64_t count = 0;
    if (perf_fd < 0) {
        return 0;
    }
    ioctl(perf_fd, PERF_EVENT_IOC_DISABLE, 0);
    if (read(perf_fd, &count, sizeof(count)) != (ssize_t)sizeof(count)) {
        return 0;
    }
    return count;
}
#else
static void perf_open(void) {}
static void perf_start(void) {}
static uint64_t perf_stop(void) { return 0; }
#endif

static unsigned bench_div(void) {
    const char *s = getenv("BENCH_N");
    unsigned d = s ? (unsigned)atoi(s) : 1;
    return d == 0 ? 1 : d;
}

static void touch(void *p) {
    if (p) {
        *(volatile unsigned char *)p = 1;
    }
}

static void malloc_free(size_t size, unsigned n) {
    unsigned i;
    for (i = 0; i < n; i++) {
        void *p = malloc(size);
        touch(p);
        free(p);
    }
}

static void calloc_free(size_t size, unsigned n) {
    unsigned i;
    for (i = 0; i < n; i++) {
        void *p = calloc(1, size);
        touch(p);
        free(p);
    }
}

static void realloc_grow(size_t oldsz, size_t newsz, unsigned n) {
    unsigned i;
    for (i = 0; i < n; i++) {
        void *p = malloc(oldsz);
        if (!p) {
            continue;
        }
        touch(p);
        void *q = realloc(p, newsz);
        touch(q);
        free(q);
    }
}

static void run(const char *name, void (*fn)(unsigned), unsigned n) {
    uint64_t t0, ns, ins;
    perf_start();
    t0 = ns_now();
    fn(n);
    ns = ns_now() - t0;
    ins = perf_stop();
    printf("bench %s ns=%llu instructions=%llu\n", name, (unsigned long long)ns,
           (unsigned long long)ins);
    fflush(stdout);
}

struct named {
    const char *name;
    void (*fn)(unsigned);
    unsigned base;
};

static void w_mf16(unsigned n) { malloc_free(16, n); }
static void w_mf64(unsigned n) { malloc_free(64, n); }
static void w_mf1k(unsigned n) { malloc_free(1024, n); }
static void w_mf64k(unsigned n) { malloc_free(65536, n); }
static void w_c64(unsigned n) { calloc_free(64, n); }
static void w_rg(unsigned n) { realloc_grow(16, 4096, n); }

int main(void) {
    unsigned d = bench_div();
    struct named cases[] = {
        {"malloc-free-16", w_mf16, 2000000},
        {"malloc-free-64", w_mf64, 2000000},
        {"malloc-free-1024", w_mf1k, 400000},
        {"malloc-free-65536", w_mf64k, 20000},
        {"calloc-64", w_c64, 400000},
        {"realloc-16-4096", w_rg, 200000},
    };
    size_t i;
    perf_open();
    for (i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        run(cases[i].name, cases[i].fn, cases[i].base / d);
    }
#ifdef __linux__
    if (perf_fd >= 0) {
        close(perf_fd);
    }
#endif
    return 0;
}
