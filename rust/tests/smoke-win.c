/* Windows C ABI smoke: malloc/free round-trip against mimalloc.dll (no pthread). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void die(const char* msg) {
  fprintf(stderr, "smoke-win failed: %s\n", msg);
  abort();
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

  char* huge = (char*)malloc(2 * 1024 * 1024);
  if (!huge) die("huge");
  huge[0] = 1;
  huge[2 * 1024 * 1024 - 1] = 2;

  free(p);
  free(c);
  free(huge);
  printf("smoke-win ok\n");
  return 0;
}
