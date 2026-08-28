/* Fork, expand, deferred-free, and error-handler checks against the Rust lib. */
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include "mimalloc.h"
#include "mimalloc-override.h"

static void die(const char* msg) {
  fprintf(stderr, "process test failed: %s\n", msg);
  exit(1);
}

static int defer_hits = 0;
static int last_err = 0;

static void on_defer(bool force, unsigned long long heartbeat, void* arg) {
  (void)force;
  (void)heartbeat;
  (void)arg;
  defer_hits++;
}

static void on_err(int err, void* arg) {
  (void)arg;
  last_err = err;
}

int main(void) {
  mi_option_disable(mi_option_verbose);

  {
    void* p = mi_malloc(64);
    if (p == NULL) die("malloc");
    if (mi_expand(p, 32) != p) die("expand shrink/fit");
    if (mi_expand(p, 64) != p) die("expand same");
    if (mi_expand(p, 1u << 20) != NULL) die("expand too large");
    if (mi_expand(NULL, 16) != NULL) die("expand null");
    mi_free(p);
  }

  defer_hits = 0;
  mi_register_deferred_free(on_defer, NULL);
  mi_collect(true);
  if (defer_hits < 1) die("deferred_free collect");
  mi_register_deferred_free(NULL, NULL);

  last_err = 0;
  mi_register_error(on_err, NULL);
  void* boom = mi_calloc((size_t)-1, (size_t)-1);
  if (boom != NULL) die("calloc overflow should fail");
  if (last_err != ENOMEM) die("error handler ENOMEM");
  mi_register_error(NULL, NULL);

  void* live = malloc(128);
  if (live == NULL) die("pre-fork malloc");
  memset(live, 0x5A, 128);

  pid_t pid = fork();
  if (pid < 0) die("fork");
  if (pid == 0) {
    if (((unsigned char*)live)[0] != 0x5A) _exit(2);
    void* c = malloc(256);
    if (c == NULL) _exit(3);
    memset(c, 0x11, 256);
    c = realloc(c, 4096);
    if (c == NULL) _exit(4);
    free(c);
    free(live);
    _exit(0);
  }
  int st = 0;
  if (waitpid(pid, &st, 0) != pid) die("waitpid");
  if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) die("child status");
  if (((unsigned char*)live)[0] != 0x5A) die("parent block after fork");
  free(live);

  {
    mi_theap_t* th = mi_theap_get_default();
    mi_theap_guarded_set_size_bound(th, 0, (size_t)-1);
    mi_theap_guarded_set_sample_rate(th, 1, 1);
    void* g = mi_malloc(64);
    if (g == NULL) die("guarded malloc");
    memset(g, 0x3C, 64);
    if (mi_usable_size(g) < 64) die("guarded usable");
    mi_free(g);
    mi_theap_guarded_set_sample_rate(th, 0, 0);
  }

  last_err = 0;
  mi_register_error(on_err, NULL);
  {
    void* p = mi_malloc(32);
    if (p == NULL) die("free_size malloc");
    mi_free_size(p, (size_t)1 << 20);
    if (last_err != EINVAL) die("free_size EINVAL");
  }
  mi_register_error(NULL, NULL);

  {
    mi_heap_t* h = mi_heap_new();
    if (h == NULL) die("heap_new numa");
    mi_heap_set_numa_affinity(h, 2);
    mi_heap_set_numa_affinity(h, -1);
    mi_heap_delete(h);
  }

  mi_thread_set_in_threadpool();
  mi_collect_reduce(0);
  mi_stats_merge();
  mi_debug_show_arenas();
  mi_arenas_print();

  {
    void* p = mi_malloc(16);
    if (mi_unsafe_heap_page_is_under_utilized(NULL, p, 50)) {
      die("current page should not be under-utilized");
    }
    if (mi_unsafe_heap_page_is_under_utilized(NULL, NULL, 50)) {
      die("null under-utilized");
    }
    mi_free(p);
  }

  printf("process ok\n");
  return 0;
}
