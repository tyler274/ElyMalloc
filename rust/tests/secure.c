/* Verify always-on secure mitigations: padding, encoded free lists,
   invalid free, metadata and end-of-page guards, and sampled object guards. */
#include <errno.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include "mimalloc.h"

#if defined(__APPLE__) && defined(__aarch64__)
#define MI_TEST_SLICE_SHIFT 17
#else
#define MI_TEST_SLICE_SHIFT 16
#endif
#define MI_TEST_SLICE_MASK ~(((uintptr_t)1 << MI_TEST_SLICE_SHIFT) - 1)
#define MI_TEST_SLICE_LAST (((uintptr_t)1 << MI_TEST_SLICE_SHIFT) - 1)

static void die(const char* msg) {
  fprintf(stderr, "secure test failed: %s\n", msg);
  exit(1);
}

static int last_err = 0;

static void on_err(int err, void* arg) {
  (void)arg;
  last_err = err;
}

static int wait_child(pid_t pid) {
  int st = 0;
  if (waitpid(pid, &st, 0) != pid) {
    die("waitpid");
  }
  return st;
}

static int child_aborted(int st) {
  if (WIFEXITED(st) && WEXITSTATUS(st) == 1) {
    return 1; /* os::efault -> _exit(1) */
  }
  if (WIFSIGNALED(st)) {
    return 1;
  }
  return 0;
}

/* Linux PROT_NONE is SIGSEGV. Darwin maps KERN_PROTECTION_FAILURE to SIGBUS. */
static int child_mem_fault(int st) {
  int sig;
  if (!WIFSIGNALED(st)) {
    return 0;
  }
  sig = WTERMSIG(st);
  return sig == SIGSEGV || sig == SIGBUS;
}

int main(void) {
  mi_option_disable(mi_option_verbose);

  {
    last_err = 0;
    mi_register_error(on_err, NULL);
    void* p = mi_malloc(16);
    if (p == NULL) {
      die("malloc");
    }
    volatile unsigned char* b = (volatile unsigned char*)p;
    for (int i = 0; i < 8; i++) {
      b[16 + i] = 0xFF;
    }
    mi_free(p);
    if (last_err != EFAULT) {
      die("overflow canary");
    }
    mi_register_error(NULL, NULL);
  }

  {
    last_err = 0;
    mi_register_error(on_err, NULL);
    void* p = mi_malloc(32);
    mi_free(p);
    mi_free(p);
    if (last_err != EAGAIN) {
      die("double free");
    }
    mi_register_error(NULL, NULL);
  }

  {
    int local = 0;
    mi_free(NULL);
    mi_free((void*)(uintptr_t)0x10);
    mi_free(&local);
    void* p = mi_malloc(64);
    if (p == NULL) {
      die("malloc2");
    }
    mi_free((char*)p + 8);
    if (mi_usable_size((char*)p + 8) != 0) {
      die("interior usable");
    }
    mi_free(p);
  }

  {
    void* p = mi_malloc(32);
    if (p == NULL) {
      die("malloc3");
    }
    pid_t pid = fork();
    if (pid < 0) {
      die("fork corrupt");
    }
    if (pid == 0) {
      mi_free(p);
      *(uintptr_t*)p = 1;
      void* q = mi_malloc(32);
      (void)q;
      _exit(2);
    }
    if (!child_aborted(wait_child(pid))) {
      die("corrupted free list should abort");
    }
    mi_free(p);
  }

  {
    void* p = mi_malloc(32);
    if (p == NULL) {
      die("malloc4");
    }
    uintptr_t slice = (uintptr_t)p & MI_TEST_SLICE_MASK;
    pid_t pid = fork();
    if (pid < 0) {
      die("fork guard");
    }
    if (pid == 0) {
      volatile char c = *(volatile char*)slice;
      (void)c;
      _exit(2);
    }
    int st = wait_child(pid);
    if (!child_mem_fault(st)) {
      if (WIFSIGNALED(st)) {
        fprintf(stderr, "secure test failed: metadata guard page (signal %d)\n",
                WTERMSIG(st));
      } else if (WIFEXITED(st)) {
        fprintf(stderr, "secure test failed: metadata guard page (exit %d)\n",
                WEXITSTATUS(st));
      }
      die("metadata guard page");
    }
    mi_free(p);
  }

  {
    void* p = mi_malloc(32);
    if (p == NULL) {
      die("malloc endguard");
    }
    uintptr_t slice = (uintptr_t)p & MI_TEST_SLICE_MASK;
    pid_t pid = fork();
    if (pid < 0) {
      die("fork endguard");
    }
    if (pid == 0) {
      volatile char c = *(volatile char*)(slice + MI_TEST_SLICE_LAST);
      (void)c;
      _exit(2);
    }
    int st = wait_child(pid);
    if (!child_mem_fault(st)) {
      die("end-of-page guard");
    }
    mi_free(p);
  }

  {
    mi_theap_t* th = mi_theap_get_default();
    mi_theap_guarded_set_size_bound(th, 0, (size_t)-1);
    mi_theap_guarded_set_sample_rate(th, 1, 1);
    void* g = mi_malloc(64);
    if (g == NULL) {
      die("guarded malloc");
    }
    size_t n = mi_usable_size(g);
    if (n < 64) {
      die("guarded usable");
    }
    pid_t pid = fork();
    if (pid < 0) {
      die("fork objguard");
    }
    if (pid == 0) {
      ((volatile char*)g)[n] = 1;
      _exit(2);
    }
    int st = wait_child(pid);
    if (!child_mem_fault(st)) {
      die("object overflow guard page");
    }
    mi_free(g);
    mi_theap_guarded_set_sample_rate(th, 0, 0);
  }

  printf("secure ok\n");
  return 0;
}
