/* Public-API probe for mimalloc-profile.h (C v3.5.2).
 * Upstream test/test-profile.c includes mimalloc/internal.h; this binary
 * only uses the exported hooks so it can link against the rewrite cdylib. */

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "mimalloc.h"
#include "mimalloc-profile.h"

typedef struct {
  mi_profiler_t profiler;
  uint64_t alloc_count;
  uint64_t free_count;
  size_t last_size;
  uint64_t last_upscaled;
  void* last_ptr;
} my_profiler_t;

static size_t on_alloc(mi_profiler_t* profiler, mi_profiler_sample_data_t* data, void* ptr,
                       size_t requested_size, size_t threshold, uint64_t bytes_since_last_sample,
                       const mi_heap_t* heap) {
  (void)threshold;
  (void)heap;
  my_profiler_t* prof = (my_profiler_t*)profiler;
  prof->alloc_count++;
  prof->last_ptr = ptr;
  prof->last_size = requested_size;
  prof->last_upscaled = bytes_since_last_sample;
  if (data != NULL && data->user_data_size >= sizeof(void*)) {
    data->user_data[0] = ptr;
  }
  return 16 * 1024;
}

static void on_free(mi_profiler_t* profiler, mi_profiler_sample_data_t* data, void* ptr,
                    const mi_heap_t* heap) {
  (void)data;
  (void)ptr;
  (void)heap;
  my_profiler_t* prof = (my_profiler_t*)profiler;
  prof->free_count++;
}

static my_profiler_t my_profiler = {
    {NULL, sizeof(void*), 0, &on_alloc, &on_free, NULL},
    0,
    0,
    0,
    0,
    NULL};

int main(void) {
  if (!mi_profile(&my_profiler.profiler)) {
    return 1;
  }
  mi_profiler_start(&my_profiler.profiler);
  void* sampled = NULL;
  for (int i = 0; i < 100000 && my_profiler.alloc_count == 0; i++) {
    if (sampled) {
      mi_free(sampled);
    }
    sampled = mi_malloc(1024);
  }
  if (my_profiler.alloc_count == 0 || my_profiler.last_ptr == NULL || my_profiler.last_size == 0) {
    return 2;
  }
  if (my_profiler.last_upscaled < my_profiler.last_size) {
    return 3;
  }
  mi_free(my_profiler.last_ptr);
  sampled = NULL;
  if (my_profiler.free_count == 0 || my_profiler.free_count > my_profiler.alloc_count) {
    return 4;
  }
  mi_profiler_stop(&my_profiler.profiler);
  printf("profile ok\n");
  return 0;
}
