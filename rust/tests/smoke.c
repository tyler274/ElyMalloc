#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <pthread.h>
#include <errno.h>

static void die(const char* msg) {
  fprintf(stderr, "smoke failed: %s\n", msg);
  abort();
}

static void* worker(void* arg) {
  (void)arg;
  for (int i = 0; i < 1000; i++) {
    size_t n = (size_t)(i % 200) + 1;
    char* p = (char*)malloc(n);
    if (!p) die("thread malloc");
    memset(p, (char)i, n);
    if ((unsigned char)p[0] != (unsigned char)i) die("thread write");
    p = (char*)realloc(p, n + 50);
    if (!p) die("thread realloc");
    free(p);
  }
  return NULL;
}

int main(void) {
  void* z = malloc(0);
  if (!z) die("malloc(0)");
  free(z);
  free(NULL);

  char* p = (char*)malloc(32);
  if (!p) die("malloc");
  memset(p, 0xAB, 32);
  if ((unsigned char)p[31] != 0xAB) die("write");

  p = (char*)realloc(p, 4096);
  if (!p) die("realloc grow");
  if ((unsigned char)p[0] != 0xAB) die("realloc preserve");

  int* c = (int*)calloc(16, sizeof(int));
  if (!c) die("calloc");
  for (int i = 0; i < 16; i++) {
    if (c[i] != 0) die("calloc zero");
  }

  void* a = NULL;
  int rc = posix_memalign(&a, 64, 128);
  if (rc != 0 || a == NULL) die("posix_memalign");
  if (((uintptr_t)a) % 64 != 0) die("posix_memalign align");

  char* huge = (char*)malloc(2 * 1024 * 1024);
  if (!huge) die("huge");
  huge[0] = 1;
  huge[2 * 1024 * 1024 - 1] = 2;

  char* d = strdup("hello");
  if (!d || strcmp(d, "hello") != 0) die("strdup");

  pthread_t th[4];
  for (int i = 0; i < 4; i++) {
    if (pthread_create(&th[i], NULL, worker, NULL) != 0) die("pthread_create");
  }
  for (int i = 0; i < 4; i++) {
    pthread_join(th[i], NULL);
  }

  free(p);
  free(c);
  free(a);
  free(huge);
  free(d);
  printf("smoke ok\n");
  return 0;
}
