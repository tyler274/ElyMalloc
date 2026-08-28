//! Public allocation entry points.

use crate::bin;
use crate::heap;
use crate::os;
use crate::page::{self, PAGE_MAGIC};
use crate::page_map;
use crate::tls;
use crate::{align_up, MAX_ALLOC, PADDING_SIZE, PTR_SIZE};
use core::ptr::{self, addr_of_mut};
use core::sync::atomic::{AtomicUsize, Ordering};

const BOOT_SIZE: usize = 64 * 1024;
static mut BOOT_MEM: [u8; BOOT_SIZE] = [0; BOOT_SIZE];
static BOOT_POS: AtomicUsize = AtomicUsize::new(0);

unsafe fn bootstrap_alloc(size: usize) -> *mut u8 {
    let size = crate::align_up(size.max(1), 16);
    loop {
        let pos = BOOT_POS.load(Ordering::Relaxed);
        let aligned = crate::align_up(pos, 16);
        let new_pos = aligned.saturating_add(size);
        if new_pos > BOOT_SIZE {
            return ptr::null_mut();
        }
        if BOOT_POS
            .compare_exchange_weak(pos, new_pos, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return addr_of_mut!(BOOT_MEM).cast::<u8>().add(aligned);
        }
    }
}

pub fn init() {
    crate::init();
}

#[inline]
unsafe fn heap() -> *mut heap::ThreadHeap {
    let h = tls::default_theap();
    if h.is_null() {
        os::enomem();
    }
    h
}

#[inline]
pub unsafe fn malloc(size: usize) -> *mut u8 {
    if tls::IN_BOOTSTRAP.load(Ordering::Acquire) {
        if tls::BOOTSTRAP_TID.load(Ordering::Acquire) == crate::os::gettid() {
            return bootstrap_alloc(size);
        }
        while !crate::is_init_done() {
            core::hint::spin_loop();
        }
    }
    crate::init();
    if size > MAX_ALLOC.saturating_sub(PADDING_SIZE) {
        os::enomem();
        return ptr::null_mut();
    }
    let h = heap();
    if h.is_null() {
        return ptr::null_mut();
    }
    heap::theap_malloc(h, size)
}

#[inline]
pub unsafe fn calloc(count: usize, size: usize) -> *mut u8 {
    let Some(total) = count.checked_mul(size) else {
        os::enomem();
        return ptr::null_mut();
    };
    let p = malloc(total);
    if !p.is_null() && total != 0 {
        ptr::write_bytes(p, 0, total);
    }
    p
}

pub unsafe fn realloc(p: *mut u8, newsize: usize) -> *mut u8 {
    if p.is_null() {
        return malloc(newsize);
    }
    if newsize == 0 {
        free(p);
        return malloc(0);
    }
    let usable = usable_size(p as *const u8);
    if usable >= newsize {
        return p;
    }
    let q = malloc(newsize);
    if q.is_null() {
        return ptr::null_mut();
    }
    let n = usable.min(newsize);
    ptr::copy_nonoverlapping(p, q, n);
    free(p);
    q
}

pub unsafe fn reallocf(p: *mut u8, newsize: usize) -> *mut u8 {
    let q = realloc(p, newsize);
    if q.is_null() && !p.is_null() && newsize != 0 {
        free(p);
    }
    q
}

/// In-place grow/shrink, or NULL if the existing block is too small.
pub unsafe fn expand(p: *mut u8, newsize: usize) -> *mut u8 {
    if p.is_null() {
        return ptr::null_mut();
    }
    if usable_size(p as *const u8) >= newsize {
        p
    } else {
        ptr::null_mut()
    }
}

pub unsafe fn free(p: *mut u8) {
    if p.is_null() {
        return;
    }
    crate::init();
    let page = page_map::get(p);
    if page.is_null() {
        return;
    }
    if (*page).magic != PAGE_MAGIC || !page::contains(page, p) {
        return;
    }
    if !(*page).is_guarded() && !page::is_block_start(page, p) {
        return;
    }
    if !page::check_free(page, p) {
        return;
    }
    let usable = page::stat_size(page);
    crate::stats::malloc_sub(usable);
    let owner = (*page).heap.load(core::sync::atomic::Ordering::Acquire);
    if !owner.is_null() {
        (*owner).stats.sub_malloc(usable);
    }
    if (*page).capacity == 1 {
        heap::unlink_huge(owner, page);
        page::destroy(page);
        return;
    }
    let h = tls::default_theap();
    if owner == h && !h.is_null() {
        page::push_local(page, p);
        heap::maybe_retire(h, page);
    } else {
        let th = tls::thread_heap();
        if owner == th && !th.is_null() {
            page::push_local(page, p);
            heap::maybe_retire(th, page);
        } else {
            page::push_thread_free(page, p);
        }
    }
}

pub fn usable_size(p: *const u8) -> usize {
    unsafe {
        if p.is_null() {
            return 0;
        }
        crate::init();
        let page = page_map::get(p);
        if page.is_null() || (*page).magic != PAGE_MAGIC {
            return 0;
        }
        page::usable_size(page, p)
    }
}

pub fn good_size(size: usize) -> usize {
    crate::init();
    bin::good_size(size)
}

pub unsafe fn malloc_aligned(size: usize, align: usize) -> *mut u8 {
    crate::init();
    if align == 0 || !align.is_power_of_two() {
        os::einval();
        return ptr::null_mut();
    }
    if size > MAX_ALLOC.saturating_sub(PADDING_SIZE) {
        os::enomem();
        return ptr::null_mut();
    }
    let h = heap();
    if h.is_null() {
        return ptr::null_mut();
    }
    heap::theap_malloc_aligned(h, size, align)
}

pub unsafe fn posix_memalign(out: *mut *mut u8, align: usize, size: usize) -> i32 {
    if out.is_null() {
        return libc::EINVAL;
    }
    if align < PTR_SIZE || !align.is_power_of_two() {
        os::einval();
        return libc::EINVAL;
    }
    let p = malloc_aligned(size, align);
    if p.is_null() {
        os::enomem();
        return libc::ENOMEM;
    }
    *out = p;
    0
}

pub unsafe fn memalign(align: usize, size: usize) -> *mut u8 {
    malloc_aligned(size, align)
}

pub unsafe fn aligned_alloc(align: usize, size: usize) -> *mut u8 {
    if align == 0 || !align.is_power_of_two() || size % align != 0 {
        os::einval();
        return ptr::null_mut();
    }
    malloc_aligned(size, align)
}

pub unsafe fn valloc(size: usize) -> *mut u8 {
    malloc_aligned(size, os::page_size())
}

pub unsafe fn pvalloc(size: usize) -> *mut u8 {
    let ps = os::page_size();
    malloc_aligned(align_up(size, ps), ps)
}

pub unsafe fn reallocarray(p: *mut u8, count: usize, size: usize) -> *mut u8 {
    let Some(total) = count.checked_mul(size) else {
        os::enomem();
        return ptr::null_mut();
    };
    realloc(p, total)
}

pub unsafe fn reallocarr(p: *mut *mut u8, count: usize, size: usize) -> i32 {
    if p.is_null() {
        os::einval();
        return libc::EINVAL;
    }
    let q = reallocarray(*p, count, size);
    if q.is_null() && (count != 0 && size != 0) {
        return libc::ENOMEM;
    }
    *p = q;
    0
}

pub unsafe fn strdup(s: *const libc::c_char) -> *mut libc::c_char {
    if s.is_null() {
        return ptr::null_mut();
    }
    let n = libc_strlen(s);
    let p = malloc(n + 1) as *mut libc::c_char;
    if p.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(s, p, n + 1);
    p
}

pub unsafe fn strndup(s: *const libc::c_char, n: usize) -> *mut libc::c_char {
    if s.is_null() {
        return ptr::null_mut();
    }
    let mut len = 0usize;
    while len < n && *s.add(len) != 0 {
        len += 1;
    }
    let p = malloc(len + 1) as *mut libc::c_char;
    if p.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(s, p, len);
    *p.add(len) = 0;
    p
}

unsafe fn libc_strlen(s: *const libc::c_char) -> usize {
    let mut n = 0usize;
    while *s.add(n) != 0 {
        n += 1;
    }
    n
}

pub unsafe fn collect(force: bool) {
    crate::init();
    let h = tls::default_theap();
    heap::collect_heap(h, force);
}

pub unsafe fn malloc_aligned_at(size: usize, align: usize, offset: usize) -> *mut u8 {
    crate::init();
    if align == 0 || !align.is_power_of_two() {
        os::einval();
        return ptr::null_mut();
    }
    if offset % align == 0 {
        return malloc_aligned(size, align);
    }
    let h = heap();
    if h.is_null() {
        return ptr::null_mut();
    }
    heap::theap_malloc_aligned_at(h, size, align, offset)
}

pub unsafe fn rezalloc(p: *mut u8, newsize: usize) -> *mut u8 {
    if p.is_null() {
        return calloc(1, newsize);
    }
    let old = usable_size(p as *const u8);
    if old >= newsize {
        return p;
    }
    let q = realloc(p, newsize);
    if !q.is_null() && newsize > old {
        ptr::write_bytes(q.add(old), 0, newsize - old);
    }
    q
}

pub unsafe fn rezalloc_aligned(p: *mut u8, newsize: usize, align: usize) -> *mut u8 {
    if p.is_null() {
        let q = malloc_aligned(newsize, align);
        if !q.is_null() {
            ptr::write_bytes(q, 0, newsize);
        }
        return q;
    }
    let old = usable_size(p as *const u8);
    if old >= newsize && (p as usize) % align == 0 {
        return p;
    }
    let q = malloc_aligned(newsize, align);
    if q.is_null() {
        return ptr::null_mut();
    }
    ptr::write_bytes(q, 0, newsize);
    ptr::copy_nonoverlapping(p, q, old.min(newsize));
    free(p);
    q
}

pub unsafe fn rezalloc_aligned_at(
    p: *mut u8,
    newsize: usize,
    align: usize,
    offset: usize,
) -> *mut u8 {
    if p.is_null() {
        let q = malloc_aligned_at(newsize, align, offset);
        if !q.is_null() {
            ptr::write_bytes(q, 0, newsize);
        }
        return q;
    }
    let old = usable_size(p as *const u8);
    let aligned_ok = if offset % align == 0 {
        (p as usize) % align == 0
    } else {
        (p as usize).wrapping_add(offset) % align == 0
    };
    if old >= newsize && aligned_ok {
        return p;
    }
    let q = malloc_aligned_at(newsize, align, offset);
    if q.is_null() {
        return ptr::null_mut();
    }
    ptr::write_bytes(q, 0, newsize);
    ptr::copy_nonoverlapping(p, q, old.min(newsize));
    free(p);
    q
}

pub unsafe fn umalloc(size: usize, block_size: *mut usize) -> *mut u8 {
    let p = malloc(size);
    if !block_size.is_null() {
        *block_size = if p.is_null() {
            0
        } else {
            usable_size(p as *const u8)
        };
    }
    p
}

pub unsafe fn urealloc(p: *mut u8, newsize: usize, pre: *mut usize, post: *mut usize) -> *mut u8 {
    if p.is_null() {
        let q = malloc(newsize);
        let sz = if q.is_null() {
            0
        } else {
            usable_size(q as *const u8)
        };
        if !pre.is_null() {
            *pre = 0;
        }
        if !post.is_null() {
            *post = sz;
        }
        return q;
    }
    crate::init();
    let page = page_map::get(p);
    if page.is_null()
        || (*page).magic != PAGE_MAGIC
        || (!(*page).is_guarded() && !page::is_block_start(page, p))
        || ((*page).is_guarded() && !page::contains(page, p))
    {
        if !pre.is_null() {
            *pre = 0;
        }
        if !post.is_null() {
            *post = 0;
        }
        return ptr::null_mut();
    }
    let old = (*page).block_size;
    if !pre.is_null() {
        *pre = old;
    }
    let q = realloc(p, newsize);
    if !post.is_null() {
        *post = if q.is_null() {
            0
        } else {
            usable_size(q as *const u8)
        };
    }
    q
}

pub unsafe fn ufree(p: *mut u8, block_size: *mut usize) {
    let sz = usable_size(p as *const u8);
    if !block_size.is_null() {
        *block_size = sz;
    }
    free(p);
}

pub unsafe fn realpath(
    fname: *const libc::c_char,
    resolved: *mut libc::c_char,
) -> *mut libc::c_char {
    const PATH_MAX: usize = 4096;
    if fname.is_null() {
        os::einval();
        return ptr::null_mut();
    }
    if !resolved.is_null() {
        let r = libc::realpath(fname, resolved);
        return r;
    }
    let mut buf = [0i8; PATH_MAX];
    let r = libc::realpath(fname, buf.as_mut_ptr());
    if r.is_null() {
        return ptr::null_mut();
    }
    strdup(buf.as_ptr())
}

pub unsafe fn reserve_os_memory(size: usize, commit: bool, allow_large: bool) -> i32 {
    reserve_os_memory_ex(size, commit, allow_large, false, ptr::null_mut())
}

pub unsafe fn reserve_os_memory_ex(
    size: usize,
    commit: bool,
    allow_large: bool,
    exclusive: bool,
    arena_id: *mut *mut crate::arena::Arena,
) -> i32 {
    crate::init();
    if size == 0 {
        if !arena_id.is_null() {
            *arena_id = ptr::null_mut();
        }
        return 0;
    }
    let a = crate::arena::reserve(size, commit, allow_large, exclusive);
    if a.is_null() {
        return libc::ENOMEM;
    }
    if !arena_id.is_null() {
        *arena_id = a;
    }
    0
}

pub unsafe fn manage_os_memory_ex(
    start: *mut u8,
    size: usize,
    is_committed: bool,
    is_pinned: bool,
    is_zero: bool,
    numa_node: i32,
    exclusive: bool,
    arena_id: *mut *mut crate::arena::Arena,
) -> bool {
    crate::init();
    let a = crate::arena::manage(
        start,
        size,
        is_committed,
        is_pinned,
        is_zero,
        numa_node,
        exclusive,
    );
    if a.is_null() {
        return false;
    }
    if !arena_id.is_null() {
        *arena_id = a;
    }
    true
}

pub const VERSION: i32 = 30500;
