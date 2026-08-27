//! Per-thread heap: current page per size class plus owned-page lists.

use crate::arena::{self, Arena};
use crate::bin::{self, BIN_COUNT};
use crate::page::{self, Page};
use crate::spin::SpinLock;
use crate::{os, LARGE_MAX_OBJ_SIZE, MAX_ALLOC, SLICE_SIZE};
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

#[repr(C, align(64))]
pub struct ThreadHeap {
    pub tid: u32,
    pub current: [*mut Page; BIN_COUNT],
    pub lists: [*mut Page; BIN_COUNT],
    pub huge: *mut Page,
    pub next_meta: *mut ThreadHeap,
    pub arena: *mut Arena,
    pub owner: *mut Heap,
}

pub type Theap = ThreadHeap;

pub const HEAP_MAGIC: u32 = 0x4D494850;

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
    unsafe {
        let bytes = BIN_COUNT * core::mem::size_of::<*mut Page>();
        let raw = os::mmap_anon(bytes);
        if raw.is_null() {
            os::abort();
        }
        match ABANDONED.compare_exchange(
            ptr::null_mut(),
            raw as *mut AtomicPtr<Page>,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            Ok(_) => raw as *mut AtomicPtr<Page>,
            Err(cur) => {
                os::munmap(raw, bytes);
                cur
            }
        }
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
        let chunk = os::mmap_anon(META_CHUNK);
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
        let chunk = os::mmap_anon(META_CHUNK);
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
    let page = abandoned(bin).swap(ptr::null_mut(), Ordering::AcqRel);
    if page.is_null() {
        return ptr::null_mut();
    }
    // The stack may hold a chain via `next`.
    let rest = (*page).next;
    (*page).next = ptr::null_mut();
    (*page).prev = ptr::null_mut();
    if !rest.is_null() {
        abandoned(bin).store(rest, Ordering::Release);
    }
    (*page).heap.store(h, Ordering::Release);
    (*page).set_abandoned(false);
    page::collect(page);
    list_push(&mut (*h).lists[bin], page);
    page
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
    list_push(&mut (*h).lists[bin], page);
    page
}

pub unsafe fn malloc_bin(h: *mut ThreadHeap, bin: usize) -> *mut u8 {
    let block_size = bin::bin_size(bin);
    let mut page = (*h).current[bin];
    if !page.is_null() {
        page::collect(page);
        let p = page::pop_local(page);
        if !p.is_null() {
            return p;
        }
    }
    // Try other owned pages of this bin.
    page = (*h).lists[bin];
    let mut n = 0;
    while !page.is_null() && n < 8 {
        page::collect(page);
        let p = page::pop_local(page);
        if !p.is_null() {
            (*h).current[bin] = page;
            return p;
        }
        page = (*page).next;
        n += 1;
    }
    page = new_page(h, bin, block_size);
    if page.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    (*h).current[bin] = page;
    let p = page::pop_local(page);
    if p.is_null() {
        os::enomem();
    }
    p
}

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
    list_push(&mut (*h).huge, page);
    let p = page::pop_local(page);
    if p.is_null() {
        list_remove(&mut (*h).huge, page);
        page::destroy(page);
        os::enomem();
        return ptr::null_mut();
    }
    p
}

pub unsafe fn unlink_huge(h: *mut ThreadHeap, page: *mut Page) {
    if h.is_null() || page.is_null() {
        return;
    }
    list_remove(&mut (*h).huge, page);
}

pub unsafe fn create() -> *mut ThreadHeap {
    let h = meta_alloc();
    if h.is_null() {
        return ptr::null_mut();
    }
    (*h).tid = os::gettid();
    h
}

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
    (*inner).owner = h;
    crate::stats::heap_add();
    register_heap(h);
    h
}

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
    h
}

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
pub unsafe fn heap_is_ok(h: *const Heap) -> bool {
    !h.is_null() && (*h).magic == HEAP_MAGIC
}

#[inline]
pub unsafe fn heap_inner(h: *mut Heap) -> *mut ThreadHeap {
    if !heap_is_ok(h) {
        return ptr::null_mut();
    }
    if (*h).is_main || (*h).inner.is_null() {
        return crate::tls::thread_heap();
    }
    (*h).inner
}

pub unsafe fn heap_theap(h: *mut Heap) -> *mut Theap {
    if h.is_null() {
        return crate::tls::thread_heap();
    }
    heap_inner(h)
}

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

pub unsafe fn heap_contains(h: *const Heap, p: *const u8) -> bool {
    if p.is_null() || !heap_is_ok(h) {
        return false;
    }
    heap_of(p) == h as *mut Heap
}

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
}

pub unsafe fn heap_delete(h: *mut Heap) {
    if !heap_is_ok(h) {
        return;
    }
    if (*h).is_main {
        return;
    }
    let dest = crate::tls::thread_heap();
    let _g = (*h).lock.lock();
    migrate_thread_heap((*h).inner, dest);
    meta_free((*h).inner);
    (*h).inner = ptr::null_mut();
    drop(_g);
    crate::stats::heap_sub();
    unregister_heap(h);
    free_heap_obj(h);
}

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

pub unsafe fn theap_malloc(th: *mut ThreadHeap, size: usize) -> *mut u8 {
    if th.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    if size > MAX_ALLOC {
        os::enomem();
        return ptr::null_mut();
    }
    let bin = bin::bin_for_size(size);
    if bin >= crate::BIN_HUGE {
        malloc_huge(th, size.max(1), 16)
    } else {
        malloc_bin(th, bin)
    }
}

pub unsafe fn theap_malloc_aligned(th: *mut ThreadHeap, size: usize, align: usize) -> *mut u8 {
    if th.is_null() {
        os::enomem();
        return ptr::null_mut();
    }
    if align == 0 || !align.is_power_of_two() {
        os::einval();
        return ptr::null_mut();
    }
    if size > MAX_ALLOC {
        os::enomem();
        return ptr::null_mut();
    }
    if align >= SLICE_SIZE {
        return malloc_huge(th, size.max(1), align);
    }
    let mut need = size.max(align);
    loop {
        let bin = bin::bin_for_size(need);
        if bin >= crate::BIN_HUGE {
            return malloc_huge(th, size.max(align).max(1), align);
        }
        let bs = bin::bin_size(bin);
        if bs % align == 0 {
            let p = malloc_bin(th, bin);
            if p.is_null() || (p as usize) % align == 0 {
                return p;
            }
            let page = crate::page_map::get(p);
            page::push_local(page, p);
            return malloc_huge(th, size.max(1), align);
        }
        need = bs.saturating_add(1);
        if need > crate::LARGE_MAX_OBJ_SIZE {
            return malloc_huge(th, size.max(align).max(1), align);
        }
    }
}

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
    meta_free(h);
}

pub unsafe fn force_unlock_meta() {
    META_LOCK.force_unlock();
    crate::arena::force_unlock();
    crate::subproc::force_unlock();
}

pub unsafe fn theap_set_default(theap: *mut Theap) -> *mut Theap {
    crate::tls::set_default_theap(theap)
}

pub unsafe fn theap_get_default() -> *mut Theap {
    crate::tls::default_theap()
}

/// Matches C `mi_heap_area_t`.
#[repr(C)]
pub struct HeapArea {
    pub blocks: *mut u8,
    pub reserved: usize,
    pub committed: usize,
    pub used: usize,
    pub block_size: usize,
    pub full_block_size: usize,
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
    let mut mapped: *mut u8 = ptr::null_mut();
    let mut mapped_bytes = 0usize;
    if need_words <= WORDS {
        bits = stack.as_mut_ptr();
    } else {
        mapped_bytes = need_words.saturating_mul(core::mem::size_of::<u64>());
        mapped = os::mmap_anon(mapped_bytes);
        if mapped.is_null() {
            return false;
        }
        bits = mapped as *mut u64;
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
        b = (*b).next;
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
    if !mapped.is_null() {
        os::munmap(mapped, mapped_bytes);
    }
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
pub unsafe fn collect_heap(h: *mut ThreadHeap, force: bool) {
    if h.is_null() {
        return;
    }
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
                os::madvise_dontneed((*page).area, (*page).map_size.saturating_sub(SLICE_SIZE / 8));
            }
            page = next;
        }
    }
}
