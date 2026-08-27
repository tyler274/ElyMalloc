/* Public theap + exclusive-arena smoke test against the Rust libmimalloc. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include "mimalloc.h"
#include "mimalloc-stats.h"

static void die(const char* msg) {
  fprintf(stderr, "theap test failed: %s\n", msg);
  exit(1);
}

static bool visit_count(const mi_heap_t* heap, const mi_heap_area_t* area, void* block, size_t block_size, void* arg) {
  (void)heap; (void)area; (void)block_size;
  if (block != NULL) {
    size_t* n = (size_t*)arg;
    (*n)++;
  }
  return true;
}

static bool count_heaps(mi_heap_t* heap, void* arg) {
  (void)heap;
  size_t* n = (size_t*)arg;
  (*n)++;
  return true;
}

int main(void) {
  mi_option_disable(mi_option_verbose);

  mi_heap_t* h = mi_heap_new();
  if (h == NULL) die("heap_new");
  mi_theap_t* t = mi_heap_theap(h);
  if (t == NULL) die("heap_theap");
  int* p = (int*)mi_theap_malloc(t, sizeof(int) * 4);
  if (p == NULL) die("theap_malloc");
  p[0] = 42;
  if (!mi_heap_contains(h, p)) die("heap_contains");
  if (mi_heap_of(p) != h) die("heap_of");

  mi_theap_t* prev = mi_theap_set_default(t);
  void* q = malloc(32);
  if (q == NULL) die("malloc via default theap");
  if (!mi_heap_contains(h, q)) die("default theap malloc not in heap");
  free(q);
  mi_theap_set_default(prev);

  mi_heap_destroy(h);

  mi_arena_id_t arena = NULL;
  if (mi_reserve_os_memory_ex(2 * 1024 * 1024, true, false, true, &arena) != 0) {
    die("reserve_os_memory_ex");
  }
  if (arena == NULL) die("arena id");
  mi_heap_t* ha = mi_heap_new_in_arena(arena);
  if (ha == NULL) die("heap_new_in_arena");
  void* r = mi_heap_malloc(ha, 128);
  if (r == NULL) die("arena heap malloc");
  if (!mi_arena_contains(arena, r)) die("arena_contains");
  mi_heap_destroy(ha);

  {
    mi_heap_t* hv = mi_heap_new();
    void* live[3];
    for (int i = 0; i < 3; i++) {
      live[i] = mi_heap_malloc(hv, 40);
      if (live[i] == NULL) die("visit malloc");
    }
    size_t seen = 0;
    if (!mi_heap_visit_blocks(hv, true, visit_count, &seen)) die("visit_blocks");
    if (seen != 3) die("visit count");
    mi_heap_destroy(hv);
  }

  {
    void* mem = NULL;
    if (posix_memalign(&mem, 64 * 1024, 2 * 1024 * 1024) != 0 || mem == NULL) {
      die("posix_memalign manage backing");
    }
    mi_arena_id_t managed = NULL;
    if (!mi_manage_os_memory_ex(mem, 2 * 1024 * 1024, true, false, false, -1, true, &managed)) {
      die("manage_os_memory_ex");
    }
    mi_heap_t* hm = mi_heap_new_in_arena(managed);
    if (hm == NULL) die("heap in managed arena");
    void* x = mi_heap_malloc(hm, 64);
    if (x == NULL || !mi_arena_contains(managed, x)) die("managed arena alloc");
    mi_heap_destroy(hm);
  }

  {
    mi_subproc_id_t sp = mi_subproc_new();
    if (sp._mi_subproc_id == NULL) die("subproc_new");
    mi_subproc_add_current_thread(sp);
    mi_heap_t* hs = mi_heap_new();
    if (hs == NULL) die("heap in subproc");
    size_t nheaps = 0;
    if (!mi_subproc_visit_heaps(sp, count_heaps, &nheaps)) die("visit_heaps");
    if (nheaps < 1) die("subproc heaps");
    mi_heap_destroy(hs);
    mi_subproc_destroy(sp);
  }

  {
    mi_heap_t* ha = mi_heap_new();
    void* a = mi_heap_malloc_aligned_at(ha, 48, 32, 8);
    if (a == NULL || (((uintptr_t)a + 8) % 32) != 0) die("heap_malloc_aligned_at");
    char* d = mi_heap_strndup(ha, "hello world", 5);
    if (d == NULL || d[0] != 'h' || d[5] != 0) die("heap_strndup");
    void* z = mi_heap_calloc_aligned(ha, 4, 16, 32);
    if (z == NULL || ((uintptr_t)z % 32) != 0) die("heap_calloc_aligned");
    for (int i = 0; i < 64; i++) {
      if (((uint8_t*)z)[i] != 0) die("heap_calloc_aligned zero");
    }
    z = mi_heap_recalloc_aligned(ha, z, 8, 16, 32);
    if (z == NULL || ((uintptr_t)z % 32) != 0) die("heap_recalloc_aligned");
    mi_heap_destroy(ha);

    uint8_t* p = (uint8_t*)mi_calloc_aligned(32, 1, 32);
    if (p == NULL) die("calloc_aligned");
    p = (uint8_t*)mi_recalloc_aligned(p, 96, 1, 32);
    if (p == NULL) die("recalloc_aligned");
    for (int i = 0; i < 96; i++) {
      if (p[i] != 0) die("recalloc_aligned zero");
    }
    mi_free(p);

    char jbuf[256];
    if (mi_stats_get_json(sizeof(jbuf), jbuf) == NULL) die("stats_get_json");
    if (jbuf[0] != '{') die("stats json");
    if (mi_stats_get_bin_size(1) < sizeof(void*)) die("stats_get_bin_size");
  }

  if (mi_heap_main() == NULL) die("heap_main");
  printf("theap/arena ok\n");
  return 0;
}
