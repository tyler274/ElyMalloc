//! Per-thread heap (`mi_theap_t`) and first-class heap (`mi_heap_t`).
//!
//! A **theap** owns pages and is the only context that may allocate. It is
//! bound to one thread (pthread TLS). A **heap** is the user-facing object
//! (`mi_heap_new`); its inner theap is created lazily per thread.
//!
//! Fast path: `current[bin]` is the page to pop. If empty, walk `lists[bin]`
//! then allocate a new page. Huge/singleton pages hang off `huge`.
//!
//! On thread exit, empty pages are unmapped and in-use pages are *abandoned*
//! (heap pointer cleared, pushed onto a process-wide list) so a later `free`
//! from any thread still finds them via the page map.

use crate::arena::{self, Arena};
use crate::bin::{self, BIN_COUNT};
use crate::page::{self, Page};
use crate::spin::SpinLock;
use crate::stats::AllocStats;
use crate::{align_up, os, LARGE_MAX_OBJ_SIZE, MAX_ALLOC, PADDING_SIZE, SLICE_SIZE};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

/// Thread-local heap (C `mi_theap_t`). Allocate/realloc only from this thread;
/// `free` of its blocks is allowed from any thread.
#[repr(C, align(64))]
pub struct ThreadHeap {
    pub tid: u32,
    /// Fast-path page per size class (C `pages_free_direct` / current queue head).
    pub current: [*mut Page; BIN_COUNT],
    /// All owned pages of that class (C `pages[bin]` queue).
    pub lists: [*mut Page; BIN_COUNT],
    /// Singleton / huge pages (`capacity == 1`).
    pub huge: *mut Page,
    pub next_meta: *mut ThreadHeap,
    /// Exclusive arena, or null to `mmap` each page.
    pub arena: *mut Arena,
    /// First-class heap this theap belongs to (may be the implicit main heap).
    pub owner: *mut Heap,
    /// Monotonic count used as the deferred-free heartbeat (C `heartbeat`).
    pub heartbeat: AtomicU64,
    /// Calls to the generic path; every 1000, run the deferred-free hook.
    pub generic_count: AtomicU32,
    pub subproc: *mut crate::subproc::Subproc,
    pub stats: AllocStats,
    pub guarded_sample_rate: usize,
    pub guarded_sample_count: usize,
    pub guarded_size_min: usize,
    pub guarded_size_max: usize,
    pub in_threadpool: bool,
}

/// Alias matching C `mi_theap_t`.
pub type Theap = ThreadHeap;

pub const HEAP_MAGIC: u32 = 0x4D494850; // 'MIHP'

/// First-class heap (C `mi_heap_t`). The default process heap is [`heap_main`].
#[repr(C, align(64))]
pub struct Heap {
    pub magic: u32,
    pub is_main: bool,
    inner: *mut ThreadHeap,
    arena: *mut Arena,
    lock: SpinLock,
    next_free: *mut Heap,
    next_all: *mut Heap,
    subproc: *mut crate::subproc::Subproc,
    numa_node: i32,
}

static mut HEAP_BUMP: *mut u8 = ptr::null_mut();
static mut HEAP_END: *mut u8 = ptr::null_mut();
static mut HEAP_FREE: *mut Heap = ptr::null_mut();

static META_LOCK: SpinLock = SpinLock::new();
static mut META_BUMP: *mut u8 = ptr::null_mut();
static mut META_END: *mut u8 = ptr::null_mut();
static mut META_FREE: *mut ThreadHeap = ptr::null_mut();

unsafe fn register_heap(h: *mut Heap) {
    if h.is_null() {
        return;
    }
    loop {
        let old = ALL_HEAPS.load(Ordering::Acquire);
        (*h).next_all = old;
        if ALL_HEAPS
            .compare_exchange_weak(old, h, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

unsafe fn unregister_heap(h: *mut Heap) {
    if h.is_null() {
        return;
    }
    let _g = META_LOCK.lock();
    let mut prev: *mut Heap = ptr::null_mut();
    let mut cur = ALL_HEAPS.load(Ordering::Acquire);
    while !cur.is_null() {
        let next = (*cur).next_all;
        if cur == h {
            if prev.is_null() {
                ALL_HEAPS.store(next, Ordering::Release);
            } else {
                (*prev).next_all = next;
            }
            (*h).next_all = ptr::null_mut();
            return;
        }
        prev = cur;
        cur = next;
    }
}

/// Walk heaps in `subproc` (null = all). Visitor returning false stops the walk.
pub unsafe fn visit_all_heaps(
    subproc: *mut crate::subproc::Subproc,
    visitor: crate::subproc::HeapVisitFun,
    arg: *mut core::ffi::c_void,
) -> bool {
    let mut cur = ALL_HEAPS.load(Ordering::Acquire);
    while !cur.is_null() {
        let next = (*cur).next_all;
        if heap_is_ok(cur) && (subproc.is_null() || (*cur).subproc == subproc) && !visitor(cur, arg)
        {
            return false;
        }
        cur = next;
    }
    true
}

static ABANDONED: AtomicPtr<AtomicPtr<Page>> = AtomicPtr::new(ptr::null_mut());
static MAIN_HEAP: AtomicPtr<Heap> = AtomicPtr::new(ptr::null_mut());
static ALL_HEAPS: AtomicPtr<Heap> = AtomicPtr::new(ptr::null_mut());

fn abandoned_table() -> *mut AtomicPtr<Page> {
    let p = ABANDONED.load(Ordering::Acquire);
    if !p.is_null() {
        return p;
    }
    let bytes = BIN_COUNT * core::mem::size_of::<*mut Page>();
    let Some(map) = os::Mapping::anon(bytes) else {
        os::abort();
    };
    let raw = map.as_ptr();
    match ABANDONED.compare_exchange(
        ptr::null_mut(),
        raw as *mut AtomicPtr<Page>,
        Ordering::Release,
        Ordering::Acquire,
    ) {
        Ok(_) => {
            let _ = map.leak();
            raw as *mut AtomicPtr<Page>
        }
        Err(cur) => cur,
    }
}

#[inline]
fn abandoned(bin: usize) -> &'static AtomicPtr<Page> {
    unsafe { &*abandoned_table().add(bin) }
}

const META_CHUNK: usize = 64 * 1024;

unsafe fn meta_alloc() -> *mut ThreadHeap {
    let _g = META_LOCK.lock();
    if !META_FREE.is_null() {
        let h = META_FREE;
        META_FREE = (*h).next_meta;
        ptr::write_bytes(h as *mut u8, 0, core::mem::size_of::<ThreadHeap>());
        return h;
    }
    let need = core::mem::size_of::<ThreadHeap>();
    if META_BUMP.is_null() || META_BUMP.add(need) > META_END {
        let chunk = os::Mapping::anon(META_CHUNK)
            .map(|m| m.leak())
            .unwrap_or(ptr::null_mut());
        if chunk.is_null() {
            return ptr::null_mut();
        }
        META_BUMP = chunk;
        META_END = chunk.add(META_CHUNK);
    }
    let h = META_BUMP as *mut ThreadHeap;
    META_BUMP = META_BUMP.add(need);
    ptr::write_bytes(h as *mut u8, 0, need);
    h
}

unsafe fn meta_free(h: *mut ThreadHeap) {
    if h.is_null() {
        return;
    }
    let _g = META_LOCK.lock();
    (*h).next_meta = META_FREE;
    META_FREE = h;
}

unsafe fn alloc_heap_obj() -> *mut Heap {
    let _g = META_LOCK.lock();
    if !HEAP_FREE.is_null() {
        let h = HEAP_FREE;
        HEAP_FREE = (*h).next_free;
        ptr::write_bytes(h as *mut u8, 0, core::mem::size_of::<Heap>());
        return h;
    }
    let need = core::mem::size_of::<Heap>();
    if HEAP_BUMP.is_null() || HEAP_BUMP.add(need) > HEAP_END {
        let chunk = os::Mapping::anon(META_CHUNK)
            .map(|m| m.leak())
            .unwrap_or(ptr::null_mut());
        if chunk.is_null() {
            return ptr::null_mut();
        }
        HEAP_BUMP = chunk;
        HEAP_END = chunk.add(META_CHUNK);
    }
    let h = HEAP_BUMP as *mut Heap;
    HEAP_BUMP = HEAP_BUMP.add(need);
    ptr::write_bytes(h as *mut u8, 0, need);
    h
}

unsafe fn free_heap_obj(h: *mut Heap) {
    if h.is_null() {
        return;
    }
    let _g = META_LOCK.lock();
    (*h).magic = 0;
    (*h).next_free = HEAP_FREE;
    HEAP_FREE = h;
}

fn list_push(head: &mut *mut Page, page: *mut Page) {
    unsafe {
        (*page).prev = ptr::null_mut();
        (*page).next = *head;
        if !(*head).is_null() {
            (**head).prev = page;
        }
        *head = page;
    }
}

fn list_remove(head: &mut *mut Page, page: *mut Page) {
    unsafe {
        let prev = (*page).prev;
        let next = (*page).next;
        if !prev.is_null() {
            (*prev).next = next;
        } else if *head == page {
            *head = next;
        }
        if !next.is_null() {
            (*next).prev = prev;
        }
        (*page).next = ptr::null_mut();
        (*page).prev = ptr::null_mut();
    }
}

unsafe fn reclaim_abandoned(h: *mut ThreadHeap, bin: usize) -> *mut Page {
    // C skips arbitrary reclaim when the thread is in a pool (blocked
    // workers should not absorb abandoned pages from other tasks).
    if (*h).in_threadpool {
        return ptr::null_mut();
    }
    // A full abandoned page (live objects, empty `local_free`) must still be
    // bound to this heap so later `free`s work. It must not be treated as a
    // fresh page: `malloc_bin` used to `ENOMEM` on `pop_local` null (NixOS
    // checkPhase / Jackett thread-pool churn).
    for _ in 0..64 {
        let taken = take_one_abandoned(h, bin);
        if taken.is_null() {
            return ptr::null_mut();
        }
        if !(*taken).local_free.is_null() {
            return taken;
        }
    }
    ptr::null_mut()
}

unsafe fn take_one_abandoned(h: *mut ThreadHeap, bin: usize) -> *mut Page {
    let page = abandoned(bin).swap(ptr::null_mut(), Ordering::AcqRel);
    if page.is_null() {
        return ptr::null_mut();
    }
    let want = (*h).subproc;
    let mut cur = page;
    let mut taken: *mut Page = ptr::null_mut();
    let mut rest_head: *mut Page = ptr::null_mut();
    let mut rest_tail: *mut Page = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        (*cur).next = ptr::null_mut();
        (*cur).prev = ptr::null_mut();
        if taken.is_null() && (*cur).subproc == want {
            taken = cur;
        } else if rest_head.is_null() {
            rest_head = cur;
            rest_tail = cur;
        } else {
            (*rest_tail).next = cur;
            rest_tail = cur;
        }
        cur = next;
    }
    if !rest_head.is_null() {
        loop {
            let old = abandoned(bin).load(Ordering::Relaxed);
            (*rest_tail).next = old;
            if abandoned(bin)
                .compare_exchange_weak(old, rest_head, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }
    if taken.is_null() {
        return ptr::null_mut();
    }
    (*taken).heap.store(h, Ordering::Release);
    (*taken).set_abandoned(false);
    page::collect(taken);
    list_push(&mut (*h).lists[bin], taken);
    taken
}

unsafe fn new_page(h: *mut ThreadHeap, bin: usize, block_size: usize) -> *mut Page {
    let reclaimed = reclaim_abandoned(h, bin);
    if !reclaimed.is_null() {
        return reclaimed;
    }
    let map_size = bin::page_size_for_block(block_size);
    let page = page::create(block_size, map_size, (*h).arena);
    if page.is_null() {
        return ptr::null_mut();
    }
    (*page).heap.store(h, Ordering::Release);
    (*page).subproc = (*h).subproc;
    (*h).stats.add_page();
    list_push(&mut (*h).lists[bin], page);
    page
}

unsafe fn maybe_deferred_free(h: *mut ThreadHeap) {
    let c = (*h).generic_count.fetch_add(1, Ordering::Relaxed) + 1;
    if c >= 1000 {
        (*h).generic_count.store(0, Ordering::Relaxed);
        let hb = (*h).heartbeat.fetch_add(1, Ordering::Relaxed) + 1;
        crate::hooks::deferred_free(false, hb);
    }
}

/// Allocate from the current page of `bin`, then other owned pages, then a new page.
pub unsafe fn malloc_bin(h: *mut ThreadHeap, bin: usize) -> *mut u8 {
    let block_size = bin::bin_size(bin);
    let mut page = (*h).current[bin];
    if !page.is_null() {
        page::collect(page);
        let p = page::pop_local(page);
        if !p.is_null() {
            crate::stats::malloc_add(block_size);
            (*h).stats.add_malloc(block_size);
            return p;
        }
    }
    maybe_deferred_free(h);
    // Collect `thread_free` on every owned page. Stopping after 8 left
    // cross-thread frees stranded (.NET / KWin) so we kept mmap'ing until
    // `nothrow new` returned null (NixOS gen 51 KWin LLVM OOM, Jackett exit 1).
    page = (*h).lists[bin];
    let mut n = 0u32;
    while !page.is_null() {
        page::collect(page);
        let p = page::pop_local(page);
        if !p.is_null() {
            (*h).current[bin] = page;
            crate::stats::malloc_add(block_size);
            (*h).stats.add_malloc(block_size);
            return p;
        }
        page = (*page).next;
        n = n.wrapping_add(1);
        if n > 1_000_000 {
            break;
        }
    }
    page = new_page(h, bin, block_size);
    if page.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    (*h).current[bin] = page;
    let p = page::pop_local(page);
    if p.is_null() {
        // Reclaim bound a full page; `new_page` should have created a fresh
        // one, but if `pop_local` is still empty, map another.
        let fresh = {
            let map_size = bin::page_size_for_block(block_size);
            page::create(block_size, map_size, (*h).arena)
        };
        if fresh.is_null() {
            os::enomem();
            return ptr::null_mut();
        }
        (*fresh).heap.store(h, Ordering::Release);
        (*fresh).subproc = (*h).subproc;
        (*h).stats.add_page();
        list_push(&mut (*h).lists[bin], fresh);
        (*h).current[bin] = fresh;
        let p = page::pop_local(fresh);
        if p.is_null() {
            os::enomem();
            return ptr::null_mut();
        }
        crate::stats::malloc_add(block_size);
        (*h).stats.add_malloc(block_size);
        return p;
    }
    crate::stats::malloc_add(block_size);
    (*h).stats.add_malloc(block_size);
    p
}

/// Singleton page for objects above [`crate::LARGE_MAX_OBJ_SIZE`] or large alignment.
pub unsafe fn malloc_huge(h: *mut ThreadHeap, size: usize, align: usize) -> *mut u8 {
    malloc_huge_at(h, size, align, 0)
}

pub unsafe fn malloc_huge_at(
    h: *mut ThreadHeap,
    size: usize,
    align: usize,
    offset: usize,
) -> *mut u8 {
    let page = page::create_huge(size, align, offset, (*h).arena);
    if page.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    (*page).heap.store(h, Ordering::Release);
    (*page).subproc = (*h).subproc;
    (*h).stats.add_page();
    list_push(&mut (*h).huge, page);
    let p = page::pop_local(page);
    if p.is_null() {
        list_remove(&mut (*h).huge, page);
        page::destroy(page);
        os::enomem();
        return ptr::null_mut();
    }
    crate::stats::malloc_add((*page).block_size);
    (*h).stats.add_malloc((*page).block_size);
    p
}

pub unsafe fn unlink_huge(h: *mut ThreadHeap, page: *mut Page) {
    if h.is_null() || page.is_null() {
        return;
    }
    list_remove(&mut (*h).huge, page);
}

#[inline]
unsafe fn should_guard(h: *mut ThreadHeap, size: usize) -> bool {
    if h.is_null() {
        return false;
    }
    let count = (*h).guarded_sample_count.wrapping_sub(1);
    if count != 0 {
        (*h).guarded_sample_count = count;
        return false;
    }
    let rate = (*h).guarded_sample_rate;
    if rate == 0 {
        return false;
    }
    if size >= (*h).guarded_size_min && size <= (*h).guarded_size_max {
        (*h).guarded_sample_count = rate;
        true
    } else {
        (*h).guarded_sample_count = 1;
        false
    }
}

unsafe fn malloc_guarded(h: *mut ThreadHeap, size: usize, align: usize) -> *mut u8 {
    let os = os::page_size();
    let align = if align == 0 { 16 } else { align };
    let obj = align_up(page::request_size(size), 16.max(align).min(os));
    let payload =
        align_up(obj.saturating_add(core::mem::size_of::<page::Block>()), os).saturating_add(os);
    let page = page::create_huge(payload, os, 0, (*h).arena);
    if page.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    (*page).heap.store(h, Ordering::Release);
    (*page).subproc = (*h).subproc;
    (*h).stats.add_page();
    list_push(&mut (*h).huge, page);
    let raw = page::pop_local(page);
    if raw.is_null() {
        list_remove(&mut (*h).huge, page);
        page::destroy(page);
        os::enomem();
        return ptr::null_mut();
    }
    let p = page::arm_guarded(page, obj);
    if p.is_null() {
        list_remove(&mut (*h).huge, page);
        page::destroy(page);
        os::enomem();
        return ptr::null_mut();
    }
    crate::stats::malloc_add((*page).block_size);
    (*h).stats.add_malloc((*page).block_size);
    crate::stats::malloc_guarded_add();
    p
}

pub unsafe fn theap_guarded_set_sample_rate(th: *mut ThreadHeap, sample_rate: usize, seed: usize) {
    if th.is_null() {
        return;
    }
    (*th).guarded_sample_rate = sample_rate;
    (*th).guarded_sample_count = sample_rate;
    if sample_rate > 1 {
        let seed = if seed == 0 {
            (*th).heartbeat.load(Ordering::Relaxed) as usize ^ (*th).tid as usize ^ (th as usize)
        } else {
            seed
        };
        (*th).guarded_sample_count = (seed % sample_rate) + 1;
    }
}

pub unsafe fn theap_guarded_set_size_bound(th: *mut ThreadHeap, min: usize, max: usize) {
    if th.is_null() {
        return;
    }
    (*th).guarded_size_min = min;
    (*th).guarded_size_max = if min > max { min } else { max };
}

/// Create the thread's default theap (called from TLS on first malloc).
pub unsafe fn create() -> *mut ThreadHeap {
    let h = meta_alloc();
    if h.is_null() {
        return ptr::null_mut();
    }
    (*h).tid = os::gettid();
    (*h).subproc = crate::subproc::current_ptr();
    (*h).guarded_size_max = 1 << 30; // 1 GiB, matching C `mi_option_guarded_max`
    h
}

/// `mi_heap_new`: a heap whose theap is created on first use.
pub unsafe fn heap_new() -> *mut Heap {
    crate::init();
    let inner = create();
    if inner.is_null() {
        return ptr::null_mut();
    }
    let h = alloc_heap_obj();
    if h.is_null() {
        meta_free(inner);
        return ptr::null_mut();
    }
    (*h).magic = HEAP_MAGIC;
    (*h).is_main = false;
    (*h).inner = inner;
    (*h).arena = ptr::null_mut();
    (*h).subproc = crate::subproc::current_ptr();
    (*h).numa_node = -1;
    (*inner).owner = h;
    (*inner).subproc = (*h).subproc;
    crate::stats::heap_add();
    register_heap(h);
    h
}

/// `mi_heap_new_in_arena`: pages come from `arena` instead of `mmap`.
pub unsafe fn heap_new_in_arena(arena: *mut Arena) -> *mut Heap {
    if !arena::is_valid(arena) {
        os::einval();
        return ptr::null_mut();
    }
    let h = heap_new();
    if h.is_null() {
        return ptr::null_mut();
    }
    (*h).arena = arena;
    (*(*h).inner).arena = arena;
    if !(*arena).subproc.is_null() {
        (*h).subproc = (*arena).subproc;
        (*(*h).inner).subproc = (*arena).subproc;
    }
    h
}

/// Process default heap (`mi_heap_main`). Created once, never deleted.
pub unsafe fn heap_main() -> *mut Heap {
    crate::init();
    let existing = MAIN_HEAP.load(Ordering::Acquire);
    if !existing.is_null() {
        return existing;
    }
    let h = alloc_heap_obj();
    if h.is_null() {
        return ptr::null_mut();
    }
    (*h).magic = HEAP_MAGIC;
    (*h).is_main = true;
    (*h).inner = ptr::null_mut();
    (*h).arena = ptr::null_mut();
    (*h).subproc = crate::subproc::main().ptr;
    (*h).numa_node = -1;
    match MAIN_HEAP.compare_exchange(ptr::null_mut(), h, Ordering::Release, Ordering::Acquire) {
        Ok(_) => {
            register_heap(h);
            h
        }
        Err(cur) => {
            free_heap_obj(h);
            cur
        }
    }
}

#[inline]
/// True if `h` is a live heap (magic + inner theap).
pub unsafe fn heap_is_ok(h: *const Heap) -> bool {
    !h.is_null() && (*h).magic == HEAP_MAGIC
}

#[inline]
/// Inner theap without creating one (`NULL` if this thread has not used `h`).
pub unsafe fn heap_inner(h: *mut Heap) -> *mut ThreadHeap {
    if !heap_is_ok(h) {
        return ptr::null_mut();
    }
    if (*h).is_main || (*h).inner.is_null() {
        return crate::tls::thread_heap();
    }
    (*h).inner
}

/// Theap of `h` on this thread, creating one if needed (`mi_heap_get_default` path).
pub unsafe fn heap_theap(h: *mut Heap) -> *mut Theap {
    if h.is_null() {
        return crate::tls::thread_heap();
    }
    heap_inner(h)
}

/// Heap that owns `p`, via the page map (`mi_heap_of`).
pub unsafe fn heap_of(p: *const u8) -> *mut Heap {
    crate::init();
    if p.is_null() {
        return ptr::null_mut();
    }
    let page = crate::page_map::get(p);
    if page.is_null() || (*page).magic != page::PAGE_MAGIC {
        return ptr::null_mut();
    }
    let th = (*page).heap.load(Ordering::Acquire);
    if th.is_null() {
        return heap_main();
    }
    let owner = (*th).owner;
    if owner.is_null() || (*owner).magic != HEAP_MAGIC {
        return heap_main();
    }
    owner
}

/// `mi_heap_contains_block`: `p` is in a page whose owner is `h`.
pub unsafe fn heap_contains(h: *const Heap, p: *const u8) -> bool {
    if p.is_null() || !heap_is_ok(h) {
        return false;
    }
    heap_of(p) == h as *mut Heap
}

/// `mi_is_in_heap_region`: page map has a live page for `p`.
pub unsafe fn any_heap_contains(p: *const u8) -> bool {
    !heap_of(p).is_null()
}

unsafe fn destroy_thread_heap_pages(th: *mut ThreadHeap) {
    if th.is_null() {
        return;
    }
    for bin in 0..BIN_COUNT {
        let mut page = (*th).lists[bin];
        (*th).lists[bin] = ptr::null_mut();
        (*th).current[bin] = ptr::null_mut();
        while !page.is_null() {
            let next = (*page).next;
            page::destroy(page);
            page = next;
        }
    }
    let mut page = (*th).huge;
    (*th).huge = ptr::null_mut();
    while !page.is_null() {
        let next = (*page).next;
        page::destroy(page);
        page = next;
    }
}

unsafe fn migrate_thread_heap(src: *mut ThreadHeap, dst: *mut ThreadHeap) {
    if src.is_null() || dst.is_null() {
        return;
    }
    for bin in 0..BIN_COUNT {
        let mut page = (*src).lists[bin];
        (*src).lists[bin] = ptr::null_mut();
        (*src).current[bin] = ptr::null_mut();
        while !page.is_null() {
            let next = (*page).next;
            (*page).next = ptr::null_mut();
            (*page).prev = ptr::null_mut();
            (*page).heap.store(dst, Ordering::Release);
            list_push(&mut (*dst).lists[bin], page);
            page = next;
        }
    }
    let mut page = (*src).huge;
    (*src).huge = ptr::null_mut();
    while !page.is_null() {
        let next = (*page).next;
        (*page).next = ptr::null_mut();
        (*page).prev = ptr::null_mut();
        (*page).heap.store(dst, Ordering::Release);
        list_push(&mut (*dst).huge, page);
        page = next;
    }
    (*dst).stats.merge_from(&(*src).stats);
}

/// `mi_heap_delete`: migrate pages to the main heap, then free the heap object.
pub unsafe fn heap_delete(h: *mut Heap) {
    if !heap_is_ok(h) {
        return;
    }
    if (*h).is_main {
        return;
    }
    let dest = crate::tls::thread_heap();
    let _g = (*h).lock.lock();
    if !dest.is_null() && !(*h).inner.is_null() {
        migrate_thread_heap((*h).inner, dest);
    }
    meta_free((*h).inner);
    (*h).inner = ptr::null_mut();
    drop(_g);
    crate::stats::heap_sub();
    unregister_heap(h);
    free_heap_obj(h);
}

/// `mi_heap_destroy`: unmap all pages (leaks user pointers), then free the heap.
pub unsafe fn heap_destroy(h: *mut Heap) {
    if !heap_is_ok(h) {
        return;
    }
    if (*h).is_main {
        return;
    }
    let _g = (*h).lock.lock();
    destroy_thread_heap_pages((*h).inner);
    meta_free((*h).inner);
    (*h).inner = ptr::null_mut();
    drop(_g);
    crate::stats::heap_sub();
    unregister_heap(h);
    free_heap_obj(h);
}

/// Allocate `size` bytes from `th` (C `_mi_theap_malloc`).
pub unsafe fn theap_malloc(th: *mut ThreadHeap, size: usize) -> *mut u8 {
    if th.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    if size > MAX_ALLOC.saturating_sub(PADDING_SIZE) {
        os::enomem();
        return ptr::null_mut();
    }
    if should_guard(th, size) {
        let p = malloc_guarded(th, size, 16);
        if !p.is_null() {
            return p;
        }
    }
    let need = page::padded_need(size);
    let bin = bin::bin_for_size(need);
    let p = if bin >= crate::BIN_HUGE {
        malloc_huge(th, need.max(1), 16)
    } else {
        malloc_bin(th, bin)
    };
    page::finish_alloc(p, page::request_size(size))
}

/// Aligned allocate. Size classes whose `block_size` is a multiple of `align`
/// are used when possible; otherwise a singleton page.
pub unsafe fn theap_malloc_aligned(th: *mut ThreadHeap, size: usize, align: usize) -> *mut u8 {
    if th.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    let align = if align == 0 { 16 } else { align };
    if !align.is_power_of_two() {
        os::einval();
        return ptr::null_mut();
    }
    if size > MAX_ALLOC.saturating_sub(PADDING_SIZE) {
        os::enomem();
        return ptr::null_mut();
    }
    if align < SLICE_SIZE && should_guard(th, size) {
        let p = malloc_guarded(th, size, align);
        if !p.is_null() {
            return p;
        }
    }
    if align >= SLICE_SIZE {
        return page::finish_alloc(
            malloc_huge(th, page::padded_need(size).max(1), align),
            page::request_size(size),
        );
    }
    // Do not use `malloc(size)` for align<=16: 8- and 24-byte classes are not
    // 16-aligned, and size=0 must still honor the requested alignment.
    let mut need = page::padded_need(size).max(align);
    loop {
        let bin = bin::bin_for_size(need);
        if bin >= crate::BIN_HUGE {
            return page::finish_alloc(
                malloc_huge(th, need.max(1), align),
                page::request_size(size),
            );
        }
        let bs = bin::bin_size(bin);
        if bs % align == 0 {
            let p = malloc_bin(th, bin);
            if p.is_null() || (p as usize) % align == 0 {
                return page::finish_alloc(p, page::request_size(size));
            }
            let pg = crate::page_map::get(p);
            crate::stats::malloc_sub((*pg).block_size);
            (*th).stats.sub_malloc((*pg).block_size);
            page::push_local(pg, p);
            return page::finish_alloc(
                malloc_huge(th, page::padded_need(size).max(1), align),
                page::request_size(size),
            );
        }
        need = bs.saturating_add(1);
        if need > LARGE_MAX_OBJ_SIZE {
            return page::finish_alloc(
                malloc_huge(th, page::padded_need(size).max(align).max(1), align),
                page::request_size(size),
            );
        }
    }
}

/// `mi_heap_malloc`.
pub unsafe fn heap_malloc(h: *mut Heap, size: usize) -> *mut u8 {
    if !heap_is_ok(h) {
        os::enomem();
        return ptr::null_mut();
    }
    if (*h).is_main {
        return theap_malloc(crate::tls::thread_heap(), size);
    }
    let _g = (*h).lock.lock();
    theap_malloc((*h).inner, size)
}

/// `mi_heap_malloc_aligned`.
pub unsafe fn heap_malloc_aligned(h: *mut Heap, size: usize, align: usize) -> *mut u8 {
    if !heap_is_ok(h) {
        os::enomem();
        return ptr::null_mut();
    }
    if (*h).is_main {
        return theap_malloc_aligned(crate::tls::thread_heap(), size, align);
    }
    let _g = (*h).lock.lock();
    theap_malloc_aligned((*h).inner, size, align)
}

pub unsafe fn theap_malloc_aligned_at(
    th: *mut ThreadHeap,
    size: usize,
    align: usize,
    offset: usize,
) -> *mut u8 {
    if th.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    if align == 0 || !align.is_power_of_two() {
        os::einval();
        return ptr::null_mut();
    }
    if offset % align == 0 {
        return theap_malloc_aligned(th, size, align);
    }
    page::finish_alloc(
        malloc_huge_at(th, page::padded_need(size).max(1), align, offset),
        page::request_size(size),
    )
}

pub unsafe fn heap_malloc_aligned_at(
    h: *mut Heap,
    size: usize,
    align: usize,
    offset: usize,
) -> *mut u8 {
    if !heap_is_ok(h) {
        os::enomem();
        return ptr::null_mut();
    }
    if (*h).is_main {
        return theap_malloc_aligned_at(crate::tls::thread_heap(), size, align, offset);
    }
    let _g = (*h).lock.lock();
    theap_malloc_aligned_at((*h).inner, size, align, offset)
}

pub unsafe fn heap_collect(h: *mut Heap, force: bool) {
    if !heap_is_ok(h) {
        return;
    }
    if (*h).is_main {
        collect_heap(crate::tls::thread_heap(), force);
        return;
    }
    let _g = (*h).lock.lock();
    collect_heap((*h).inner, force);
}

pub unsafe fn theap_collect(th: *mut ThreadHeap, force: bool) {
    collect_heap(th, force);
}

/// Unmap an empty page unless it is the current-page cache for its bin.
pub unsafe fn maybe_retire(h: *mut ThreadHeap, page: *mut Page) {
    if page.is_null() {
        return;
    }
    if (*page).used != 0 {
        return;
    }
    page::collect(page);
    if (*page).used != 0 {
        return;
    }
    let bin = if (*page).capacity == 1 && (*page).block_size as usize > LARGE_MAX_OBJ_SIZE {
        BIN_COUNT - 1
    } else {
        bin::bin_for_size((*page).block_size as usize)
    };
    if bin < BIN_COUNT && (*h).current[bin] == page {
        // Keep one empty page as a cache.
        return;
    }
    if bin < BIN_COUNT {
        list_remove(&mut (*h).lists[bin], page);
        if (*h).current[bin] == page {
            (*h).current[bin] = (*h).lists[bin];
        }
    }
    page::destroy(page);
}

/// Thread is exiting: abandon in-use pages, free empty ones, recycle the heap.
pub unsafe fn abandon(h: *mut ThreadHeap) {
    if h.is_null() {
        return;
    }
    crate::alloc::flush_quarantine();
    for bin in 0..BIN_COUNT {
        let mut page = (*h).lists[bin];
        (*h).lists[bin] = ptr::null_mut();
        (*h).current[bin] = ptr::null_mut();
        while !page.is_null() {
            let next = (*page).next;
            (*page).next = ptr::null_mut();
            (*page).prev = ptr::null_mut();
            page::collect(page);
            if (*page).used == 0 {
                page::destroy(page);
            } else {
                (*page).heap.store(ptr::null_mut(), Ordering::Release);
                (*page).set_abandoned(true);
                loop {
                    let old = abandoned(bin).load(Ordering::Relaxed);
                    (*page).next = old;
                    if abandoned(bin)
                        .compare_exchange_weak(old, page, Ordering::Release, Ordering::Relaxed)
                        .is_ok()
                    {
                        break;
                    }
                }
            }
            page = next;
        }
    }
    let mut page = (*h).huge;
    (*h).huge = ptr::null_mut();
    while !page.is_null() {
        let next = (*page).next;
        (*page).next = ptr::null_mut();
        (*page).prev = ptr::null_mut();
        if (*page).used == 0 {
            page::destroy(page);
        } else {
            (*page).heap.store(ptr::null_mut(), Ordering::Release);
            (*page).set_abandoned(true);
        }
        page = next;
    }
    meta_free(h);
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub unsafe fn force_unlock_meta() {
    META_LOCK.force_unlock();
    crate::arena::force_unlock();
    crate::subproc::force_unlock();
}

/// Reset every heap lock after `fork` in the child.
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub unsafe fn force_unlock_all() {
    force_unlock_meta();
    let mut cur = ALL_HEAPS.load(Ordering::Acquire);
    while !cur.is_null() {
        let next = (*cur).next_all;
        (*cur).lock.force_unlock();
        cur = next;
    }
}

/// `mi_heap_set_default` / `mi_heap_get_default` (theap, not the first-class heap).
pub unsafe fn theap_set_default(theap: *mut Theap) -> *mut Theap {
    crate::tls::set_default_theap(theap)
}

/// Current default theap (`mi_heap_get_default`).
pub unsafe fn theap_get_default() -> *mut Theap {
    crate::tls::default_theap()
}

/// `mi_heap_area_t`: one page as seen by `mi_heap_visit_blocks`.
#[repr(C)]
/// Layout-compatible with C `mi_heap_area_t`.
#[repr(C)]
pub struct HeapArea {
    /// Start of the block area.
    pub blocks: *mut u8,
    pub reserved: usize,
    pub committed: usize,
    /// Live blocks (`used` after collect is approximate if `thread_free` is unmerged).
    pub used: usize,
    pub block_size: usize,
    pub full_block_size: usize,
    /// This rewrite stores the `Page*` here for the visitor.
    pub reserved1: *mut core::ffi::c_void,
}

pub type BlockVisitFun = unsafe extern "C" fn(
    heap: *const Heap,
    area: *const HeapArea,
    block: *mut core::ffi::c_void,
    block_size: usize,
    arg: *mut core::ffi::c_void,
) -> bool;

unsafe fn area_from_page(page: *mut Page) -> HeapArea {
    let bs = (*page).block_size;
    let cap = (*page).capacity as usize;
    HeapArea {
        blocks: (*page).area,
        reserved: (*page).map_size,
        committed: cap.saturating_mul(bs),
        used: (*page).used as usize,
        block_size: bs,
        full_block_size: bs,
        reserved1: page as *mut core::ffi::c_void,
    }
}

unsafe fn visit_page_blocks(
    heap: *const Heap,
    page: *mut Page,
    visit_blocks: bool,
    visitor: BlockVisitFun,
    arg: *mut core::ffi::c_void,
) -> bool {
    if page.is_null() {
        return true;
    }
    page::collect(page);
    let area = area_from_page(page);
    if !visit_blocks {
        return visitor(heap, &area, ptr::null_mut(), 0, arg);
    }
    if (*page).used == 0 {
        return true;
    }
    let cap = (*page).capacity as usize;
    let bs = (*page).block_size;
    let start = (*page).area;
    if cap == 1 {
        return visitor(heap, &area, start as *mut core::ffi::c_void, bs, arg);
    }

    const WORDS: usize = 64;
    let mut stack = [0u64; WORDS];
    let need_words = cap.div_ceil(64);
    let bits: *mut u64;
    let mut mapping: Option<os::Mapping> = None;
    if need_words <= WORDS {
        bits = stack.as_mut_ptr();
    } else {
        let mapped_bytes = need_words.saturating_mul(core::mem::size_of::<u64>());
        mapping = os::Mapping::anon(mapped_bytes);
        let Some(ref m) = mapping else {
            return false;
        };
        bits = m.as_ptr() as *mut u64;
    }

    let mut b = (*page).local_free;
    while !b.is_null() {
        let off = (b as usize).wrapping_sub(start as usize);
        if bs != 0 && off % bs == 0 {
            let idx = off / bs;
            if idx < cap {
                *bits.add(idx / 64) |= 1u64 << (idx % 64);
            }
        }
        b = page::block_next(page, b);
    }

    let mut ok = true;
    for i in 0..cap {
        if (*bits.add(i / 64) & (1u64 << (i % 64))) != 0 {
            continue;
        }
        let p = start.add(i * bs);
        if !visitor(heap, &area, p as *mut core::ffi::c_void, bs, arg) {
            ok = false;
            break;
        }
    }
    drop(mapping);
    ok
}

unsafe fn visit_theap_pages(
    heap: *const Heap,
    th: *mut ThreadHeap,
    visit_blocks: bool,
    visitor: BlockVisitFun,
    arg: *mut core::ffi::c_void,
) -> bool {
    if th.is_null() {
        return true;
    }
    for bin in 0..BIN_COUNT {
        let mut page = (*th).lists[bin];
        while !page.is_null() {
            let next = (*page).next;
            if !visit_page_blocks(heap, page, visit_blocks, visitor, arg) {
                return false;
            }
            page = next;
        }
    }
    let mut page = (*th).huge;
    while !page.is_null() {
        let next = (*page).next;
        if !visit_page_blocks(heap, page, visit_blocks, visitor, arg) {
            return false;
        }
        page = next;
    }
    true
}

/// `mi_theap_visit_blocks`.
pub unsafe fn theap_visit_blocks(
    th: *mut ThreadHeap,
    visit_blocks: bool,
    visitor: Option<BlockVisitFun>,
    arg: *mut core::ffi::c_void,
) -> bool {
    let Some(visitor) = visitor else {
        return true;
    };
    let heap = if th.is_null() || (*th).owner.is_null() {
        heap_main()
    } else {
        (*th).owner
    };
    visit_theap_pages(heap, th, visit_blocks, visitor, arg)
}

pub unsafe fn heap_visit_blocks(
    h: *mut Heap,
    visit_blocks: bool,
    visitor: Option<BlockVisitFun>,
    arg: *mut core::ffi::c_void,
) -> bool {
    let Some(visitor) = visitor else {
        return true;
    };
    if !heap_is_ok(h) {
        return false;
    }
    if (*h).is_main {
        return visit_theap_pages(h, crate::tls::thread_heap(), visit_blocks, visitor, arg);
    }
    let _g = (*h).lock.lock();
    visit_theap_pages(h, (*h).inner, visit_blocks, visitor, arg)
}

pub unsafe fn heap_visit_abandoned_blocks(
    h: *mut Heap,
    visit_blocks: bool,
    visitor: Option<BlockVisitFun>,
    arg: *mut core::ffi::c_void,
) -> bool {
    let Some(visitor) = visitor else {
        return true;
    };
    let heap = if heap_is_ok(h) { h } else { heap_main() };
    let table = abandoned_table();
    if table.is_null() {
        return true;
    }
    for bin in 0..BIN_COUNT {
        let mut page = (*table.add(bin)).load(Ordering::Acquire);
        while !page.is_null() {
            let next = (*page).next;
            if !visit_page_blocks(heap, page, visit_blocks, visitor, arg) {
                return false;
            }
            page = next;
        }
    }
    true
}

/// Best-effort purge of empty pages on the current heap.
/// Best-effort purge of empty pages on the current heap (C `mi_collect`).
pub unsafe fn collect_heap(h: *mut ThreadHeap, force: bool) {
    if h.is_null() {
        return;
    }
    if force {
        crate::alloc::flush_quarantine();
    }
    let hb = (*h).heartbeat.fetch_add(1, Ordering::Relaxed) + 1;
    crate::hooks::deferred_free(force, hb);
    for bin in 0..BIN_COUNT {
        let mut page = (*h).lists[bin];
        while !page.is_null() {
            let next = (*page).next;
            page::collect(page);
            if (*page).used == 0 && (force || (*h).current[bin] != page) {
                list_remove(&mut (*h).lists[bin], page);
                if (*h).current[bin] == page {
                    (*h).current[bin] = (*h).lists[bin];
                }
                page::destroy(page);
            } else if (*page).used == 0 {
                let n = ((*page).capacity as usize).saturating_mul((*page).block_size);
                if n >= os::min_purge_size() {
                    os::purge((*page).area, n);
                }
            }
            page = next;
        }
    }
}

pub unsafe fn heap_stats_get(h: *mut Heap, out: *mut crate::stats::Stats) -> bool {
    if out.is_null() || !heap_is_ok(h) {
        return false;
    }
    if (*h).is_main {
        crate::stats::fill(out);
        return true;
    }
    crate::stats::clear(out);
    if !(*h).inner.is_null() {
        (*(*h).inner).stats.copy_into(out);
    }
    true
}

pub unsafe fn theap_stats_get(th: *mut ThreadHeap, out: *mut crate::stats::Stats) -> bool {
    if out.is_null() || th.is_null() {
        return false;
    }
    crate::stats::clear(out);
    (*th).stats.copy_into(out);
    true
}

pub unsafe fn heap_stats_add_into(h: *mut Heap, out: *mut crate::stats::Stats) {
    if h.is_null() || out.is_null() || !heap_is_ok(h) {
        return;
    }
    if (*h).is_main {
        return;
    }
    if !(*h).inner.is_null() {
        (*(*h).inner).stats.add_into(out);
    }
}

pub unsafe fn heap_stats_merge_to_subproc(h: *mut Heap) {
    if !heap_is_ok(h) || (*h).inner.is_null() || (*h).subproc.is_null() {
        return;
    }
    (*(*h).subproc).stats.take_from(&(*(*h).inner).stats);
}

/// `mi_heap_set_numa_affinity`. Negative `numa_node` means any node.
pub unsafe fn heap_set_numa_affinity(h: *mut Heap, numa_node: i32) {
    crate::init();
    let h = if h.is_null() { heap_main() } else { h };
    if !heap_is_ok(h) {
        return;
    }
    (*h).numa_node = if numa_node < 0 { -1 } else { numa_node };
}

pub unsafe fn heap_numa_node(h: *const Heap) -> i32 {
    if !heap_is_ok(h) {
        return -1;
    }
    (*h).numa_node
}

/// Hint that this theap is used from a thread pool (C `mi_thread_set_in_threadpool`).
pub unsafe fn theap_set_in_threadpool(th: *mut ThreadHeap) {
    if th.is_null() {
        return;
    }
    (*th).in_threadpool = true;
}

/// C `mi_unsafe_heap_page_is_under_utilized`: skip the current-queue head
/// (list `prev == NULL`) to avoid immediate thrashing.
pub unsafe fn page_is_under_utilized(heap: *mut Heap, p: *const u8, perc_threshold: usize) -> bool {
    if p.is_null() {
        return false;
    }
    crate::init();
    let page = crate::page_map::get(p);
    if page.is_null() || (*page).magic != page::PAGE_MAGIC {
        return false;
    }
    if (*page).used == (*page).capacity {
        return false;
    }
    if (*page).prev.is_null() {
        return false;
    }
    let th = (*page).heap.load(Ordering::Acquire);
    if th.is_null() {
        return false;
    }
    let page_heap = if (*th).owner.is_null() || (*(*th).owner).magic != HEAP_MAGIC {
        heap_main()
    } else {
        (*th).owner
    };
    if page_heap.is_null() {
        return false;
    }
    if !heap.is_null() && heap != page_heap {
        return false;
    }
    let cap = (*page).capacity as usize;
    if cap == 0 {
        return false;
    }
    if perc_threshold >= 100 {
        return true;
    }
    perc_threshold >= (100 * (*page).used as usize) / cap
}

/// Force-collect every heap (inspired `mi_collect_reduce`).
pub unsafe fn collect_all(force: bool) {
    crate::init();
    collect_heap(crate::tls::default_theap(), force);
    let mut cur = ALL_HEAPS.load(Ordering::Acquire);
    while !cur.is_null() {
        let next = (*cur).next_all;
        if heap_is_ok(cur) {
            heap_collect(cur, force);
        }
        cur = next;
    }
}

/// Merge the current thread's theap stats into its subprocess and reset.
/// Matches C `mi_stats_merge` (thread-local, not every heap).
pub unsafe fn stats_merge() {
    crate::init();
    let th = crate::tls::default_theap();
    if th.is_null() || (*th).subproc.is_null() {
        return;
    }
    (*(*th).subproc).stats.take_from(&(*th).stats);
}

/// Destroy non-main heaps tagged with `s` (`mi_subproc_delete` path).
pub unsafe fn destroy_heaps_in_subproc(s: *mut crate::subproc::Subproc) {
    if s.is_null() {
        return;
    }
    loop {
        let mut cur = ALL_HEAPS.load(Ordering::Acquire);
        let mut found: *mut Heap = ptr::null_mut();
        while !cur.is_null() {
            let next = (*cur).next_all;
            if heap_is_ok(cur) && !(*cur).is_main && (*cur).subproc == s {
                found = cur;
                break;
            }
            cur = next;
        }
        if found.is_null() {
            break;
        }
        heap_destroy(found);
    }
}
