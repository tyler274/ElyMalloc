//! Mimalloc pages: one size class, local + concurrent free lists (C `page.c`).
//!
//! A *page* is a 64 KiB / 512 KiB / 4 MiB mapping (or a singleton huge block)
//! holding equal-sized objects. Metadata sits in the mapping behind OS
//! `PROT_NONE` guard pages (C `MI_SECURE` / `MI_SECURE=FULL`): lead and mid
//! around the page header, plus an end-of-page guard (`MI_SECURE>=5`).
//!
//! # Free lists
//!
//! C `mi_page_t` keeps three lists:
//!
//! - `free` - ready for `malloc` (this rewrite pops `local_free` after
//!   [`collect`] merges `thread_free`).
//! - `local_free` - blocks freed by the owning thread; not yet on the alloc path.
//! - `thread_free` - blocks freed by other threads (lock-free push).
//!
//! Accounting (C `types.h`):
//!
//! ```text
//! used - |thread_free|                 == live blocks
//! used - |thread_free| + |free| + |local_free| == capacity
//! ```
//!
//! Only the owning thread mutates non-atomic fields. Concurrent `free`
//! pushes onto `thread_free`. An abandoned page (`heap == NULL`) is still
//! findable via the page map so a later `free` can recycle or destroy it.
//!
//! # Encoding
//!
//! `Block.next` is not a raw pointer. C `mi_ptr_encode` uses per-page keys
//! `k1, k2`: `((p ^ k2) <<< k1) + k1`. XOR-only encoding leaks `k1` if `p`
//! is guessable; the rotate+add is non-associative. Null is encoded as the
//! page address so `(k2 <<< k1) + k1` is not a common sentinel. Decode that
//! is not a block start in this page is treated as heap corruption (`EFAULT`).
//! On `malloc`, C zeros `block->next` so the encoding is not leaked to the
//! caller; [`pop_local`] does the same.

use crate::arena::{self, Arena};
use crate::mem;
use crate::os;
use crate::page_map;
use crate::ptrx;
use crate::{align_up, PADDING_SIZE, PTR_SIZE, SLICE_SIZE};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

/// One allocated or free object. Overlay: the first word is the encoded next
/// pointer while the block is on a free list (C `mi_block_t`).
#[repr(C)]
pub struct Block {
    /// Encoded next pointer (see [`block_next`] / [`block_set_next`]).
    pub next: usize,
}

/// Mimalloc page header (C `mi_page_t`). Cache-line aligned; lives in the
/// mapping at `map_base + OS_PAGE`, between lead and mid guard pages.
#[repr(C, align(64))]
pub struct Page {
    /// `PAGE_MAGIC` (`'MIPA'`) while the page is live.
    pub magic: u32,
    /// Bytes per block, including padding. Always `> 0`.
    pub block_size: usize,
    /// Number of blocks carved from [`Self::area`].
    pub capacity: u32,
    /// Blocks not on `local_free` (includes those still on `thread_free`).
    pub used: u32,
    /// Owning-thread free list (encoded `next`). `malloc` pops this.
    pub local_free: *mut Block,
    /// Concurrent free list from other threads (C `xthread_free`, without the ownership bit).
    pub thread_free: AtomicPtr<Block>,
    /// Next page of the same size class on the owning theap (or abandoned list).
    pub next: *mut Page,
    /// Previous page in that doubly-linked list.
    pub prev: *mut Page,
    /// Owning theap, or null if abandoned (C `theap == NULL`).
    pub heap: AtomicPtr<crate::heap::ThreadHeap>,
    /// First block. Every block is `block_size` bytes from here.
    pub area: *mut u8,
    /// `mmap` / arena base, including guards. Page-map keys use this range.
    pub map_base: *mut u8,
    /// Bytes from [`Self::map_base`]; multiple of [`crate::SLICE_SIZE`].
    pub map_size: usize,
    pub flags: AtomicU32,
    /// Per-page encode keys (C `page->keys[]`). `key1` is odd.
    pub key1: usize,
    pub key2: usize,
    pub subproc: *mut crate::subproc::Subproc,
}

/// Live-page sentinel (`'MIPA'`). Cleared implicitly by unmap.
pub const PAGE_MAGIC: u32 = 0x4D495041; // 'MIPA'
const FLAG_ABANDONED: u32 = 1;
const FLAG_ARENA: u32 = 2;
const FLAG_GUARDED: u32 = 4;

/// Match C `MI_DEBUG_UNINIT` / `MI_DEBUG_FREED` / `MI_DEBUG_PADDING`.
pub const DEBUG_UNINIT: u8 = 0xD0;
pub const DEBUG_FREED: u8 = 0xDF;
pub const DEBUG_PADDING: u8 = 0xDE;
const DEBUG_FILL_MAX: usize = 1024 * 1024;
/// C `mi_ptr_encode_canary_freed`: bit 9 set so it cannot match a live canary.
const CANARY_FREED: u32 = 0x00DEAD00;
/// C `MI_BLOCK_TAG_GUARDED`: `block.next` of a sampled guarded allocation.
const BLOCK_TAG_GUARDED: usize = usize::MAX;

/// Trailer at `block + block_size - 8` (C `mi_padding_t`).
///
/// `canary` is a truncated encode of the block pointer (lowest byte cleared so
/// a one-byte overflow is not a valid canary). `delta` is slack between the
/// user size and the trailer: `usable = block_size - PADDING_SIZE - delta`.
#[repr(C)]
struct Padding {
    canary: u32,
    delta: u32,
}

#[inline]
fn fill_enabled() -> bool {
    cfg!(any(debug_assertions, feature = "debug-fill"))
}

/// C `MI_PADDING_CHECK_BYTES` (`MI_SECURE>=5` or debug): fill slack between the
/// user size and the canary trailer so a short overflow is visible on `free`.
#[inline]
fn padding_check_bytes() -> bool {
    true
}

#[inline]
fn debug_fill(p: *mut u8, size: usize, byte: u8) {
    if fill_enabled() && !p.is_null() && size != 0 {
        unsafe {
            mem::fill(p, byte, size);
        }
    }
}

impl Page {
    #[inline]
    pub fn set_abandoned(&self, yes: bool) {
        if yes {
            self.flags.fetch_or(FLAG_ABANDONED, Ordering::Release);
        } else {
            self.flags.fetch_and(!FLAG_ABANDONED, Ordering::Release);
        }
    }

    /// True if the mapping came from an exclusive arena and must not be `munmap`'d.
    #[inline]
    pub fn is_arena(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & FLAG_ARENA != 0
    }

    #[inline]
    fn set_arena(&self) {
        self.flags.fetch_or(FLAG_ARENA, Ordering::Release);
    }

    /// Sampled allocation with a `PROT_NONE` OS page after the object (C `MI_GUARDED`).
    #[inline]
    pub fn is_guarded(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & FLAG_GUARDED != 0
    }

    #[inline]
    pub fn set_guarded(&self) {
        self.flags.fetch_or(FLAG_GUARDED, Ordering::Release);
    }
}

static KEY_SEQ: AtomicU64 = AtomicU64::new(0x9E37_79B9);

unsafe fn init_keys(page: *mut Page) {
    let n = KEY_SEQ.fetch_add(0x9E37, Ordering::Relaxed) as usize;
    let a = ptrx::addr_mut(page);
    (*page).key1 = a
        .wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as usize)
        .wrapping_add(n)
        | 1;
    (*page).key2 = (*page).key1.rotate_left(13) ^ n.wrapping_mul(0xA076_1D64_78BD_642Fu64 as usize);
}

#[inline]
fn rotl(x: usize, k: usize) -> usize {
    x.rotate_left((k & (usize::BITS as usize - 1)) as u32)
}

#[inline]
fn rotr(x: usize, k: usize) -> usize {
    x.rotate_right((k & (usize::BITS as usize - 1)) as u32)
}

/// Encode a pointer address into the free-list next field (C `mi_ptr_encode`).
/// Integer-only so Kani can prove the roundtrip without OS mappings.
///
/// `null` is represented as the page address (passed in as `addr` by the
/// caller), not the integer 0, so the encoded sentinel is not `(k2 <<< k1)+k1`.
#[inline]
pub(crate) fn encode_addr(key1: usize, key2: usize, addr: usize) -> usize {
    rotl(addr ^ key2, key1).wrapping_add(key1)
}

/// Inverse of [`encode_addr`].
#[inline]
pub(crate) fn decode_addr(key1: usize, key2: usize, enc: usize) -> usize {
    rotr(enc.wrapping_sub(key1), key1) ^ key2
}

#[inline]
unsafe fn encode_ptr(page: *const Page, p: *mut Block) -> usize {
    let x = if p.is_null() {
        ptrx::addr(page)
    } else {
        ptrx::addr_mut(p)
    };
    encode_addr((*page).key1, (*page).key2, x)
}

/// Decode `block.next` and abort if the pointer is outside this page.
///
/// # Safety
/// `page` and `block` must be live; `block` belongs to `page`.
#[inline]
pub unsafe fn block_next(page: *mut Page, block: *mut Block) -> *mut Block {
    if page.is_null() || block.is_null() {
        return ptr::null_mut();
    }
    let decoded = decode_addr((*page).key1, (*page).key2, (*block).next);
    if decoded == ptrx::addr(page as *const Page) {
        return ptr::null_mut();
    }
    // Encrypted next pointers are addresses, not live references.
    let p: *mut Block = ptrx::with_exposed(decoded);
    if !contains(page, p.cast()) || !is_block_start(page, p.cast()) {
        os::efault();
    }
    p
}

/// Encode `next` (or the page address for null) into `block.next`.
#[inline]
pub unsafe fn block_set_next(page: *mut Page, block: *mut Block, next: *mut Block) {
    if block.is_null() {
        return;
    }
    (*block).next = encode_ptr(page, next);
}

unsafe fn map_page_memory(size: usize, align: usize, arena: *mut Arena) -> (*mut u8, bool) {
    if !arena.is_null() {
        let p = arena::alloc(arena, size, align);
        if !p.is_null() {
            return (p, true);
        }
        if arena::is_valid(arena) && (*arena).exclusive {
            return (ptr::null_mut(), true);
        }
    }
    (os::mmap_aligned(size, align), false)
}

unsafe fn unmap_page_memory(base: *mut u8, size: usize, from_arena: bool) {
    if from_arena {
        return;
    }
    os::munmap(base, size);
}

/// Align the first block so every block is aligned to the largest power of two
/// that divides `block_size` (needed for `malloc_aligned` over-size classes).
///
/// C `MI_PAGE_MIN_START_BLOCK_ALIGN` is 16; power-of-two sizes then stay
/// naturally aligned up to 4 KiB (`MI_PAGE_MAX_START_BLOCK_ALIGN2`).
fn block_align(block_size: usize) -> usize {
    if block_size == 0 {
        return 16;
    }
    let po2 = block_size & block_size.wrapping_neg();
    po2.max(16)
}

/// `[lead guard][Page][mid guard][blocks…][end guard]` - C `MI_SECURE` / `MI_SECURE=FULL`.
fn meta_prefix(block_align: usize) -> (usize, usize, usize) {
    let os = os::page_size();
    let lead = os;
    let meta = align_up(core::mem::size_of::<Page>(), os);
    let mid = os;
    let area0 = lead + meta + mid;
    (lead, meta, align_up(area0, block_align.max(1)))
}

#[inline]
fn end_guard_size() -> usize {
    os::page_size()
}

unsafe fn install_meta_guards(base: *mut u8, lead: usize, meta: usize) {
    let os = os::page_size();
    let _ = os::protect(base, os);
    let _ = os::protect(base.add(lead + meta), os);
}

unsafe fn install_end_guard(base: *mut u8, map_size: usize) {
    let os = end_guard_size();
    if base.is_null() || map_size <= os {
        return;
    }
    let _ = os::protect(base.add(map_size - os), os);
}

unsafe fn unprotect_meta_guards(page: *mut Page) {
    if page.is_null() || (*page).map_base.is_null() {
        return;
    }
    let os = os::page_size();
    let base = (*page).map_base;
    let lead = os;
    let meta = align_up(core::mem::size_of::<Page>(), os);
    let _ = os::unprotect(base, os);
    let _ = os::unprotect(base.add(lead + meta), os);
    let size = (*page).map_size;
    if size > os {
        let _ = os::unprotect(base.add(size - os), os);
    }
}

fn shuffle_usize(x: usize) -> usize {
    x.wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as usize)
        .rotate_left(13)
        ^ (x >> 7)
}

/// Randomized free list (C `mi_page_free_list_extend_secure`, `MI_SECURE>=2`).
unsafe fn init_local_free(page: *mut Page, area: *mut u8, bsize: usize, capacity: usize) {
    const MAX_SLICES: usize = 64;
    (*page).local_free = ptr::null_mut();
    if capacity == 0 || area.is_null() || bsize == 0 {
        return;
    }
    if capacity < 4 {
        let mut i = capacity;
        while i > 0 {
            i -= 1;
            let b = area.add(i * bsize) as *mut Block;
            block_set_next(page, b, (*page).local_free);
            (*page).local_free = b;
        }
        return;
    }
    let mut shift = 6usize;
    while (capacity >> shift) == 0 {
        shift -= 1;
    }
    let slice_count = 1usize << shift;
    let slice_extend = capacity / slice_count;
    let mut blocks = [ptr::null_mut::<Block>(); MAX_SLICES];
    let mut counts = [0usize; MAX_SLICES];
    let mut i = 0;
    while i < slice_count {
        blocks[i] = area.add(i * slice_extend * bsize) as *mut Block;
        counts[i] = slice_extend;
        i += 1;
    }
    counts[slice_count - 1] += capacity % slice_count;

    let mut rnd = (*page).key1 | 1;
    let mut current = rnd % slice_count;
    counts[current] -= 1;
    let free_start = blocks[current];
    rnd = shuffle_usize(rnd);
    i = 1;
    while i < capacity {
        let round = i % PTR_SIZE;
        if round == 0 {
            rnd = shuffle_usize(rnd);
        }
        let mut next = (rnd >> (8 * round)) & (slice_count - 1);
        while counts[next] == 0 {
            next += 1;
            if next == slice_count {
                next = 0;
            }
        }
        counts[next] -= 1;
        let block = blocks[current];
        blocks[current] = (block as *mut u8).add(bsize) as *mut Block;
        block_set_next(page, block, blocks[next]);
        current = next;
        i += 1;
    }
    block_set_next(page, blocks[current], ptr::null_mut());
    (*page).local_free = free_start;
}

/// Allocate a page of `map_size` bytes holding equal `block_size` blocks.
///
/// Mapping: `[lead PROT_NONE][Page][mid PROT_NONE][blocks…][end PROT_NONE]`.
/// Registers every 64 KiB slice of `map_base..+map_size` in the page map.
pub unsafe fn create(block_size: usize, map_size: usize, arena: *mut Arena) -> *mut Page {
    let align = block_align(block_size);
    let (lead, meta, area_off) = meta_prefix(align);
    let tail = end_guard_size();
    let need = area_off.saturating_add(block_size).saturating_add(tail);
    let map_size = align_up(map_size.max(SLICE_SIZE).max(need), SLICE_SIZE);
    let (base, from_arena) = map_page_memory(map_size, SLICE_SIZE, arena);
    if base.is_null() {
        return ptr::null_mut();
    }
    if area_off.saturating_add(block_size).saturating_add(tail) > map_size {
        unmap_page_memory(base, map_size, from_arena);
        return ptr::null_mut();
    }
    let page = base.add(lead) as *mut Page;
    ptr::write_bytes(page as *mut u8, 0, core::mem::size_of::<Page>());
    let area = base.add(area_off);
    let usable = map_size.saturating_sub(area_off).saturating_sub(tail);
    let capacity = usable / block_size;
    if capacity == 0 {
        unmap_page_memory(base, map_size, from_arena);
        return ptr::null_mut();
    }

    (*page).magic = PAGE_MAGIC;
    (*page).block_size = block_size;
    (*page).capacity = capacity as u32;
    (*page).used = 0;
    (*page).local_free = ptr::null_mut();
    (*page).thread_free = AtomicPtr::new(ptr::null_mut());
    (*page).next = ptr::null_mut();
    (*page).prev = ptr::null_mut();
    (*page).heap = AtomicPtr::new(ptr::null_mut());
    (*page).area = area;
    (*page).map_base = base;
    (*page).map_size = map_size;
    (*page).flags = AtomicU32::new(0);
    if from_arena {
        (*page).set_arena();
    }
    init_keys(page);
    init_local_free(page, area, block_size, capacity);
    install_meta_guards(base, lead, meta);
    install_end_guard(base, map_size);

    page_map::set_range(base, map_size, page);
    crate::stats::page_add();
    page
}

/// Dedicated huge/aligned allocation: one block covering `size` bytes (plus header).
///
/// C `MI_PAGE_SINGLETON`: objects `> MI_LARGE_MAX_OBJ_SIZE` or alignment
/// `> MI_PAGE_MAX_OVERALLOC_ALIGN` (64 KiB). `offset` is for `malloc_aligned_at`.
pub unsafe fn create_huge(
    size: usize,
    align: usize,
    offset: usize,
    arena: *mut Arena,
) -> *mut Page {
    let align = if align == 0 { 16 } else { align };
    if !align.is_power_of_two() {
        return ptr::null_mut();
    }
    // Over-allocate to the next power of two (min one 64 KiB slice) so
    // programs that `realloc` in small steps (kmod/xz grows a .ko 8 KiB
    // at a time) can stay in-place instead of mmap+copy+munmap each call.
    let need = size.max(16);
    let payload = align_up(
        need.checked_next_power_of_two()
            .unwrap_or(need)
            .max(SLICE_SIZE),
        SLICE_SIZE,
    );
    let (lead, meta, prefix) = meta_prefix(16);
    let tail = end_guard_size();
    let total = align_up(
        prefix
            .saturating_add(align)
            .saturating_add(payload)
            .saturating_add(tail),
        SLICE_SIZE.max(align.min(SLICE_SIZE * 16)),
    );
    let map_align = align.max(SLICE_SIZE);
    let (base, from_arena) = map_page_memory(total, map_align, arena);
    if base.is_null() {
        return ptr::null_mut();
    }
    let page = base.add(lead) as *mut Page;
    ptr::write_bytes(page as *mut u8, 0, core::mem::size_of::<Page>());
    let area0 = (base as usize) + prefix;
    let off = offset % align;
    let want = if off == 0 { 0 } else { align - off };
    let cur = area0 % align;
    let add = (want + align - cur) % align;
    let area = (area0 + add) as *mut u8;
    if (area as usize) + payload + tail > (base as usize) + total {
        unmap_page_memory(base, total, from_arena);
        return ptr::null_mut();
    }
    let area_end = (base as usize) + total - tail;
    let block_bytes = area_end.saturating_sub(area as usize);
    if block_bytes < need {
        unmap_page_memory(base, total, from_arena);
        return ptr::null_mut();
    }
    (*page).magic = PAGE_MAGIC;
    (*page).block_size = block_bytes;
    (*page).capacity = 1;
    (*page).used = 0;
    (*page).local_free = area as *mut Block;
    (*page).thread_free = AtomicPtr::new(ptr::null_mut());
    (*page).next = ptr::null_mut();
    (*page).prev = ptr::null_mut();
    (*page).heap = AtomicPtr::new(ptr::null_mut());
    (*page).area = area;
    (*page).map_base = base;
    (*page).map_size = total;
    (*page).flags = AtomicU32::new(0);
    if from_arena {
        (*page).set_arena();
    }
    init_keys(page);
    install_meta_guards(base, lead, meta);
    install_end_guard(base, total);
    page_map::set_range(base, total, page);
    crate::stats::page_add();
    page
}

/// Unmap the page: clear the page map, drop guard protection, `munmap` unless arena-backed.
pub unsafe fn destroy(page: *mut Page) {
    if page.is_null() {
        return;
    }
    unprotect_meta_guards(page);
    unguard(page);
    let from_arena = (*page).is_arena();
    let owner = (*page).heap.load(Ordering::Acquire);
    if !owner.is_null() {
        (*owner).stats.sub_page();
    }
    let base = (*page).map_base;
    let size = (*page).map_size;
    page_map::clear_range(base, size);
    crate::stats::page_sub();
    unmap_page_memory(base, size, from_arena);
}

/// Move `thread_free` onto `local_free` and subtract from `used` (C `_mi_page_free_collect`).
///
/// After this, `used` is the live count and `local_free` is the alloc list.
/// Aborts if the concurrent list is longer than `capacity` (corruption).
#[inline]
pub unsafe fn collect(page: *mut Page) {
    if page.is_null() {
        return;
    }
    let mut p = (*page).thread_free.swap(ptr::null_mut(), Ordering::AcqRel);
    let max = (*page).capacity.max((*page).used);
    let mut n = 0u32;
    while !p.is_null() {
        n += 1;
        if n > max {
            os::efault();
        }
        let next = block_next(page, p);
        block_set_next(page, p, (*page).local_free);
        (*page).local_free = p;
        p = next;
    }
    (*page).used = (*page).used.saturating_sub(n);
}

/// Pop one block from `local_free`. Huge pages (`capacity == 1`) return `area` once.
///
/// After the pop, `block.next` is cleared (C `mi_page_malloc_zero`: `block->next = 0`)
/// so the encoded free-list pointer is not leaked to the caller.
#[inline]
pub unsafe fn pop_local(page: *mut Page) -> *mut u8 {
    if (*page).capacity == 1 {
        if (*page).used != 0 || (*page).area.is_null() {
            return ptr::null_mut();
        }
        if (*page).local_free.is_null() {
            return ptr::null_mut();
        }
        (*page).local_free = ptr::null_mut();
        (*page).used = 1;
        let p = (*page).area;
        os::reuse(p, (*page).block_size);
        clear_block_next(p);
        return p;
    }
    let b = (*page).local_free;
    if b.is_null() {
        return ptr::null_mut();
    }
    (*page).local_free = block_next(page, b);
    (*page).used = (*page).used.saturating_add(1);
    let p = b as *mut u8;
    if (*page).used == 1 {
        let n = ((*page).capacity as usize).saturating_mul((*page).block_size);
        os::reuse((*page).area, n);
    }
    clear_block_next(p);
    debug_fill(
        p,
        (*page).block_size.saturating_sub(PADDING_SIZE),
        DEBUG_UNINIT,
    );
    p
}

#[inline]
unsafe fn clear_block_next(p: *mut u8) {
    if !p.is_null() {
        (*(p as *mut Block)).next = 0;
    }
}

/// Owning-thread free: push onto `local_free` (C `mi_free_block_local`).
#[inline]
pub unsafe fn push_local(page: *mut Page, ptr: *mut u8) {
    if (*page).capacity == 1 {
        (*page).local_free = ptr as *mut Block;
        (*page).used = 0;
        return;
    }
    debug_fill(
        ptr,
        (*page)
            .block_size
            .saturating_sub(PADDING_SIZE)
            .min(DEBUG_FILL_MAX),
        DEBUG_FREED,
    );
    let b = ptr as *mut Block;
    block_set_next(page, b, (*page).local_free);
    (*page).local_free = b;
    (*page).used = (*page).used.saturating_sub(1);
}

/// Other-thread free: lock-free push onto `thread_free` (C `mi_free_block_mt`).
#[inline]
pub unsafe fn push_thread_free(page: *mut Page, ptr: *mut u8) {
    debug_fill(
        ptr,
        (*page)
            .block_size
            .saturating_sub(PADDING_SIZE)
            .min(DEBUG_FILL_MAX),
        DEBUG_FREED,
    );
    let b = ptr as *mut Block;
    loop {
        let old = (*page).thread_free.load(Ordering::Relaxed);
        block_set_next(page, b, old);
        if (*page)
            .thread_free
            .compare_exchange_weak(old, b, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

/// True if `ptr` lies in `[area, area + capacity * block_size)`.
#[inline]
pub unsafe fn contains(page: *mut Page, ptr: *const u8) -> bool {
    if page.is_null() || ptr.is_null() {
        return false;
    }
    let start = ptrx::addr((*page).area);
    let end = start + ((*page).capacity as usize) * (*page).block_size;
    let addr = ptrx::addr(ptr);
    addr >= start && addr < end
}

/// True if `ptr` is the start of a block (offset from `area` is a multiple of `block_size`).
#[inline]
pub unsafe fn is_block_start(page: *mut Page, ptr: *const u8) -> bool {
    if !contains(page, ptr) {
        return false;
    }
    let bs = (*page).block_size;
    if bs == 0 {
        return false;
    }
    let off = ptrx::addr(ptr) - ptrx::addr((*page).area);
    off % bs == 0
}

/// `malloc(0)` still needs a non-zero block; C treats 0 as one word.
#[inline]
pub fn request_size(size: usize) -> usize {
    if size == 0 {
        PTR_SIZE
    } else {
        size
    }
}

/// Size class payload including the padding trailer.
#[inline]
pub fn padded_need(size: usize) -> usize {
    request_size(size).saturating_add(PADDING_SIZE)
}

#[inline]
unsafe fn canary(page: *const Page, block: *mut u8) -> u32 {
    (encode_ptr(page, block as *mut Block) as u32) & 0xFFFFFE00
}

unsafe fn padding_ptr(page: *mut Page, block: *mut u8) -> *mut Padding {
    let bsize = (*page).block_size.saturating_sub(PADDING_SIZE);
    block.add(bsize) as *mut Padding
}

/// Write the `{canary, delta}` trailer after a successful allocation (C `_mi_padding`).
pub unsafe fn write_padding(p: *mut u8, user_size: usize) {
    if p.is_null() {
        return;
    }
    let page = page_map::get(p);
    if page.is_null() || (*page).magic != PAGE_MAGIC || (*page).is_guarded() {
        return;
    }
    if (*page).block_size < PADDING_SIZE || !is_block_start(page, p) {
        return;
    }
    let bsize = (*page).block_size - PADDING_SIZE;
    let user = user_size.min(bsize);
    let delta = bsize - user;
    let pad = padding_ptr(page, p);
    (*pad).canary = canary(page, p);
    (*pad).delta = delta as u32;
    if padding_check_bytes() && (*page).capacity != 1 && delta != 0 {
        let n = delta.min(crate::MAX_ALIGN_SIZE);
        mem::fill(p.add(user), DEBUG_PADDING, n);
    }
}

/// Stamp padding on a freshly popped block. No-op on null (OOM).
pub unsafe fn finish_alloc(p: *mut u8, user_size: usize) -> *mut u8 {
    if !p.is_null() {
        write_padding(p, user_size);
    }
    p
}

/// Grow (or shrink) in the existing block if the size class / huge mapping
/// has room. Updates the padding trailer so `usable_size` stays exact.
/// Guarded objects only succeed when `newsize` already fits before the OS page.
pub unsafe fn try_realloc_in_place(p: *mut u8, newsize: usize) -> bool {
    if p.is_null() || newsize == 0 {
        return false;
    }
    let page = page_map::get(p);
    if page.is_null() || (*page).magic != PAGE_MAGIC {
        return false;
    }
    if (*page).is_guarded() {
        return usable_size(page, p) >= newsize;
    }
    if (*page).block_size < PADDING_SIZE || !is_block_start(page, p) {
        return false;
    }
    let bsize = (*page).block_size - PADDING_SIZE;
    let need = request_size(newsize);
    if need > bsize {
        return false;
    }
    write_padding(p, need);
    true
}

unsafe fn decode_padding(page: *mut Page, block: *mut u8) -> Option<(usize, usize)> {
    if (*page).block_size < PADDING_SIZE {
        return None;
    }
    let bsize = (*page).block_size - PADDING_SIZE;
    let pad = padding_ptr(page, block);
    let delta = (*pad).delta as usize;
    if (*pad).canary == canary(page, block) && delta <= bsize {
        Some((delta, bsize))
    } else {
        None
    }
}

/// Byte-precise usable size (user request), or 0 if `p` is not a live block.
pub unsafe fn usable_size(page: *mut Page, p: *const u8) -> usize {
    if page.is_null() || p.is_null() || !contains(page, p) {
        return 0;
    }
    if (*page).is_guarded() {
        let os = os::page_size();
        let bs = (*page).block_size;
        if bs <= os {
            return 0;
        }
        let guard = ptrx::addr((*page).area) + bs - os;
        let addr = ptrx::addr(p);
        if addr >= guard {
            return 0;
        }
        return guard - addr;
    }
    if !is_block_start(page, p) {
        return 0;
    }
    match decode_padding(page, p as *mut u8) {
        Some((delta, bsize)) => bsize - delta,
        None => 0,
    }
}

/// Bytes charged to stats for this page (class / huge payload, not the user size).
pub unsafe fn stat_size(page: *mut Page) -> usize {
    if page.is_null() {
        return 0;
    }
    if (*page).is_guarded() {
        (*page).block_size.saturating_sub(os::page_size())
    } else {
        (*page).block_size
    }
}

/// Verify padding, mark the canary as freed, and report overflow / double-free.
/// Returns `false` if the block must not be recycled.
pub unsafe fn check_free(page: *mut Page, p: *mut u8) -> bool {
    if page.is_null() || p.is_null() {
        return false;
    }
    if (*page).is_guarded() {
        return true;
    }
    if !is_block_start(page, p) {
        return false;
    }
    if (*page).block_size < PADDING_SIZE {
        return false;
    }
    let bsize = (*page).block_size - PADDING_SIZE;
    let pad = padding_ptr(page, p);
    if (*pad).canary == CANARY_FREED {
        os::eagain();
        return false;
    }
    match decode_padding(page, p) {
        Some((delta, _)) => {
            if padding_check_bytes() && (*page).capacity != 1 && delta != 0 {
                let user = bsize - delta;
                let n = delta.min(crate::MAX_ALIGN_SIZE);
                if !mem::eq_filled(p.add(user), DEBUG_PADDING, n) {
                    os::efault_report();
                    return false;
                }
            }
            (*pad).canary = CANARY_FREED;
            true
        }
        None => {
            os::efault_report();
            false
        }
    }
}

/// Drop `PROT_NONE` on a guarded object's trailing OS page before `munmap`.
pub unsafe fn unguard(page: *mut Page) {
    if page.is_null() || !(*page).is_guarded() {
        return;
    }
    let os = os::page_size();
    let bs = (*page).block_size;
    if bs <= os || (*page).area.is_null() {
        return;
    }
    os::unprotect((*page).area.add(bs - os), os);
}

/// Tag a huge page as a sampled guarded allocation and protect the last OS page.
/// Returns the user pointer placed immediately before the guard page.
pub unsafe fn arm_guarded(page: *mut Page, obj_size: usize) -> *mut u8 {
    if page.is_null() || (*page).area.is_null() {
        return ptr::null_mut();
    }
    let os = os::page_size();
    let bs = (*page).block_size;
    if bs
        < obj_size
            .saturating_add(os)
            .saturating_add(core::mem::size_of::<Block>())
    {
        return ptr::null_mut();
    }
    (*page).set_guarded();
    let area = (*page).area;
    (*(area as *mut Block)).next = BLOCK_TAG_GUARDED;
    let guard = area.add(bs - os);
    let _ = os::protect(guard, os);
    let p = guard.sub(obj_size);
    if ptrx::addr(p) < ptrx::addr(area) + core::mem::size_of::<Block>() {
        os::unprotect(guard, os);
        return ptr::null_mut();
    }
    p
}
