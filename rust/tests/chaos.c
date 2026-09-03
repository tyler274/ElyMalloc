/* Seeded malloc/free/realloc/aligned chaos under LD_PRELOAD / DYLD insert.
   Invariants: alignment, usable_size >= request, realloc keeps a prefix,
   calloc is zero, live blocks do not alias. Optional pthreads + cross-thread
   free unless MIMALLOC_QEMU=1. MIMALLOC_CHAOS_STEPS / MIMALLOC_CHAOS_SEED. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "mimalloc.h"

#ifndef _WIN32
#include <pthread.h>
#endif

#define LIVE_MAX 256
#define STEPS_DEFAULT 2048

struct live {
  unsigned char* p;
  size_t n;
  size_t align;
  uint64_t tag;
};

static _Thread_local uint64_t rng_state = 1;

static uint64_t splitmix(void) {
  rng_state += 0x9E3779B97F4A7C15ull;
  uint64_t z = rng_state;
  z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
  z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
  return z ^ (z >> 31);
}

static size_t rng_size(void) {
  switch ((int)(splitmix() % 16)) {
    case 0:
      return 0;
    case 1:
      return 1;
    case 2:
      return 24;
    case 3:
      return 32;
    case 4:
      return 96;
    case 5:
      return 4096;
    default:
      return (size_t)(splitmix() % 512) + 1;
  }
}

static size_t rng_align(void) {
  return (size_t)1 << (3 + (splitmix() % 8));
}

static void die(const char* msg) {
  fprintf(stderr, "chaos: %s\n", msg);
  abort();
}

static void paint(unsigned char* p, size_t n, uint64_t tag) {
  size_t i;
  unsigned char t[8];
  memcpy(t, &tag, 8);
  for (i = 0; i < n; i++) {
    p[i] = t[i & 7];
  }
}

static void check(unsigned char* p, size_t n, uint64_t tag, const char* what) {
  size_t i;
  unsigned char t[8];
  if (p == NULL) {
    fprintf(stderr, "chaos: %s null\n", what);
    abort();
  }
  memcpy(t, &tag, 8);
  for (i = 0; i < n; i++) {
    if (p[i] != t[i & 7]) {
      fprintf(stderr, "chaos: %s corrupt at %zu\n", what, i);
      abort();
    }
  }
}

static int qemu_user(void) {
  const char* q = getenv("MIMALLOC_QEMU");
  return q != NULL && q[0] == '1' && q[1] == 0;
}

static unsigned env_u32(const char* name, unsigned def) {
  const char* s = getenv(name);
  if (s == NULL || s[0] == 0) {
    return def;
  }
  unsigned long v = strtoul(s, NULL, 0);
  if (v == 0) {
    return def;
  }
  if (v > 0x7ffffffful) {
    return 0x7fffffffu;
  }
  return (unsigned)v;
}

static void check_overlap(struct live* live, int nlive, unsigned char* p, size_t n) {
  size_t unew = mi_usable_size(p);
  uintptr_t a0 = (uintptr_t)p;
  uintptr_t a1 = a0 + (unew == 0 ? 1 : unew);
  int i;
  (void)n;
  for (i = 0; i < nlive; i++) {
    uintptr_t b0 = (uintptr_t)live[i].p;
    size_t un = mi_usable_size(live[i].p);
    uintptr_t b1 = b0 + (un == 0 ? 1 : un);
    if (!(a1 <= b0 || b1 <= a0)) {
      die("overlap");
    }
  }
}

static void push_live(struct live* live, int* nlive, unsigned char* p, size_t n, size_t align,
                      uint64_t tag) {
  if (p == NULL) {
    die("alloc");
  }
  if (((uintptr_t)p % (align ? align : 16)) != 0) {
    die("align");
  }
  if (mi_usable_size(p) < n) {
    die("usable");
  }
  check_overlap(live, *nlive, p, n);
  paint(p, n, tag);
  live[*nlive].p = p;
  live[*nlive].n = n;
  live[*nlive].align = align;
  live[*nlive].tag = tag;
  (*nlive)++;
}

static void run_ops(unsigned steps) {
  struct live live[LIVE_MAX];
  int nlive = 0;
  unsigned step;
  uint64_t tag = 1;

  for (step = 0; step < steps; step++) {
    int op = (int)(splitmix() % 8);
    if (nlive == 0) {
      op = 0;
    }
    if (nlive == LIVE_MAX) {
      op = 1;
    }
    switch (op) {
      case 0: {
        size_t n = rng_size();
        unsigned char* p = (unsigned char*)mi_malloc(n);
        if (p == NULL) {
          die("malloc");
        }
        if (((uintptr_t)p % 16) != 0) {
          die("malloc align");
        }
        tag++;
        push_live(live, &nlive, p, n, 16, tag);
        break;
      }
      case 1: {
        int i = (int)(splitmix() % (unsigned)nlive);
        check(live[i].p, live[i].n, live[i].tag, "free");
        mi_free(live[i].p);
        live[i] = live[nlive - 1];
        nlive--;
        break;
      }
      case 2: {
        int i = (int)(splitmix() % (unsigned)nlive);
        size_t nn = rng_size();
        size_t keep = live[i].n < nn ? live[i].n : nn;
        check(live[i].p, live[i].n, live[i].tag, "realloc-src");
        unsigned char* q = (unsigned char*)mi_realloc(live[i].p, nn);
        if (q == NULL && nn != 0) {
          die("realloc");
        }
        if (keep > 0) {
          check(q, keep, live[i].tag, "realloc-keep");
        }
        live[i] = live[nlive - 1];
        nlive--;
        tag++;
        push_live(live, &nlive, q, nn, 16, tag);
        break;
      }
      case 3: {
        size_t n = rng_size();
        unsigned char* p = (unsigned char*)mi_calloc(1, n);
        size_t i;
        if (p == NULL) {
          die("calloc");
        }
        for (i = 0; i < n; i++) {
          if (p[i] != 0) {
            die("calloc zero");
          }
        }
        tag++;
        push_live(live, &nlive, p, n, 16, tag);
        break;
      }
      case 4: {
        size_t a = rng_align();
        size_t n = rng_size();
        unsigned char* p = (unsigned char*)mi_malloc_aligned(n, a);
        if (p == NULL) {
          die("aligned");
        }
        tag++;
        push_live(live, &nlive, p, n, a, tag);
        break;
      }
      default:
        mi_collect(0);
        break;
    }
  }

  while (nlive > 0) {
    nlive--;
    check(live[nlive].p, live[nlive].n, live[nlive].tag, "drain");
    mi_free(live[nlive].p);
  }
}

#ifndef _WIN32
struct worker_arg {
  unsigned steps;
  uint64_t seed;
};

static void* worker(void* vp) {
  struct worker_arg* a = (struct worker_arg*)vp;
  rng_state = a->seed;
  run_ops(a->steps);
  return NULL;
}

static void* producer(void* vp) {
  unsigned char** bag = (unsigned char**)vp;
  int i;
  for (i = 0; i < 64; i++) {
    unsigned char* p = (unsigned char*)mi_malloc(32);
    if (p == NULL) {
      die("xthread malloc");
    }
    memset(p, (unsigned char)(i + 1), 32);
    bag[i] = p;
  }
  return NULL;
}
#endif

int main(void) {
  unsigned steps = env_u32("MIMALLOC_CHAOS_STEPS", STEPS_DEFAULT);
  const char* seed = getenv("MIMALLOC_CHAOS_SEED");
  if (seed != NULL && seed[0] != 0) {
    rng_state = strtoull(seed, NULL, 0);
    if (rng_state == 0) {
      rng_state = 1;
    }
  }

  mi_option_disable(mi_option_verbose);
  run_ops(steps);

#ifndef _WIN32
  if (!qemu_user()) {
    pthread_t th[4];
    struct worker_arg args[4];
    unsigned tsteps = steps / 8;
    int t;
    if (tsteps < 64) {
      tsteps = 64;
    }
    for (t = 0; t < 4; t++) {
      args[t].steps = tsteps;
      args[t].seed = rng_state + (uint64_t)(t + 1) * 0x9E3779B97F4A7C15ull;
      if (pthread_create(&th[t], NULL, worker, &args[t]) != 0) {
        die("pthread_create");
      }
    }
    for (t = 0; t < 4; t++) {
      pthread_join(th[t], NULL);
    }

        {
      unsigned char* bag[64];
      pthread_t prod;
      int i;
      memset(bag, 0, sizeof(bag));
      if (pthread_create(&prod, NULL, producer, bag) != 0) {
        die("producer");
      }
      pthread_join(prod, NULL);
      for (i = 0; i < 64; i++) {
        if (bag[i] == NULL || bag[i][0] != (unsigned char)(i + 1)) {
          die("xthread cookie");
        }
        mi_free(bag[i]);
      }
    }
  }
#endif

  printf("chaos ok\n");
  return 0;
}
