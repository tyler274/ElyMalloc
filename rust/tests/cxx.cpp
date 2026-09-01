/* C++ ABI smoke test: STL allocator + throwing new_handler. */
#if defined(__GNUC__) && !defined(__clang__)
#pragma GCC diagnostic ignored "-Walloc-size-larger-than="
#endif
#include <atomic>
#include <cstdio>
#include <new>
#include <thread>
#include <vector>
#include "mimalloc.h"

static int fails;

static void die(const char* msg) {
  std::fprintf(stderr, "cxx test failed: %s\n", msg);
  fails++;
}

int main() {
  mi_option_disable(mi_option_verbose);

  {
    std::vector<int, mi_stl_allocator<int> > vec;
    vec.push_back(1);
    vec.push_back(2);
    vec.pop_back();
    if (vec.size() != 1 || vec[0] != 1) die("stl_allocator");
  }

  {
    mi_heap_t* h = mi_heap_new();
    if (h == NULL) die("heap_new");
    void* p = mi_heap_alloc_new(h, 64);
    if (p == NULL) die("heap_alloc_new");
    static_cast<char*>(p)[0] = 7;
    mi_heap_destroy(h);
  }

  {
    void* p = mi_new_nothrow(static_cast<size_t>(-1) / 2);
    if (p != NULL) die("new_nothrow huge");
  }

  {
    // KWin/LLVM: many compiler threads, first alloc is aligned nothrow new(4096, 16).
    const int n = 64;
    std::atomic<int> ready{0};
    std::atomic<int> go{0};
    std::atomic<int> ok{0};
    std::vector<std::thread> ts;
    ts.reserve(n);
    for (int i = 0; i < n; i++) {
      ts.emplace_back([&] {
        ready.fetch_add(1, std::memory_order_relaxed);
        while (go.load(std::memory_order_acquire) == 0) {
        }
        void* p = ::operator new(4096, std::align_val_t(16), std::nothrow);
        if (p == nullptr) {
          return;
        }
        static_cast<char*>(p)[0] = 1;
        static_cast<char*>(p)[4095] = 2;
        void* z = ::operator new(64, std::align_val_t(0), std::nothrow);
        ::operator delete(p, std::align_val_t(16), std::nothrow);
        if (z == nullptr) {
          return;
        }
        ::operator delete(z, std::align_val_t(16), std::nothrow);
        ok.fetch_add(1, std::memory_order_relaxed);
      });
    }
    while (ready.load(std::memory_order_relaxed) < n) {
    }
    go.store(1, std::memory_order_release);
    for (auto& t : ts) {
      t.join();
    }
    if (ok.load(std::memory_order_relaxed) != n) die("aligned nothrow first alloc");
  }

  std::set_new_handler([] { throw std::bad_alloc(); });
  try {
    (void)mi_new_n(static_cast<size_t>(-1) / 2, 4);
    die("new_n overflow did not throw");
  } catch (const std::bad_alloc&) {
    /* expected */
  }

  if (fails) return 1;
  std::printf("cxx ok\n");
  return 0;
}
