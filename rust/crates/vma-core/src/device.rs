//! Device-memory allocator: default pools, custom pools, dedicated blocks,
//! buffer/image helpers. One `VkDeviceMemory` is a *block*; `VmaAllocation`
//! is a subregion (C `types.h` terminology).

use crate::free_list::{align_up, FreeList};
use crate::types::*;
use crate::vk::*;
use core::ffi::{c_char, c_void};
use std::sync::Mutex;

const DEFAULT_LARGE_BLOCK: u64 = 256 * 1024 * 1024;
const LARGE_HEAP: u64 = 1024 * 1024 * 1024;

pub struct Block {
    pub memory: VkDeviceMemory,
    pub size: u64,
    pub mapped: *mut c_void,
    pub map_refs: u32,
    pub free: FreeList,
    pub type_index: u32,
}

pub struct Allocation {
    pub memory: VkDeviceMemory,
    pub offset: u64,
    pub size: u64,
    pub type_index: u32,
    pub dedicated: bool,
    pub block: usize,
    pub pool: VmaPool,
    pub map_refs: u32,
    pub user_data: *mut c_void,
    pub name: Option<std::ffi::CString>,
    pub mapped: *mut c_void,
}

pub struct Pool {
    pub create: VmaPoolCreateInfo,
    pub name: Option<std::ffi::CString>,
    pub blocks: Vec<Block>,
    pub allocations: Vec<*mut Allocation>,
}

pub struct Defrag {
    pub moves: Vec<VmaDefragmentationMove>,
    pub stats: VmaDefragmentationStats,
}

pub struct Allocator {
    pub flags: u32,
    pub instance: VkInstance,
    pub physical: VkPhysicalDevice,
    pub device: VkDevice,
    pub funcs: VmaVulkanFunctions,
    pub props: VkPhysicalDeviceProperties,
    pub mem: VkPhysicalDeviceMemoryProperties,
    pub heap_limit: [u64; VK_MAX_MEMORY_HEAPS],
    pub block_size: u64,
    pub callbacks: VmaDeviceMemoryCallbacks,
    pub types: Vec<TypePool>,
    pub pools: Vec<*mut Pool>,
    pub frame: u32,
    pub lock: Mutex<()>,
}

pub struct TypePool {
    pub blocks: Vec<Block>,
    pub allocations: Vec<*mut Allocation>,
}

unsafe fn lock_if(allocator: *mut Allocator) -> Option<std::sync::MutexGuard<'static, ()>> {
    if allocator.is_null() {
        return None;
    }
    if (*allocator).flags & VMA_ALLOCATOR_CREATE_EXTERNALLY_SYNCHRONIZED_BIT != 0 {
        return None;
    }
    let lock = &(*allocator).lock as *const Mutex<()>;
    let g = (*lock).lock().unwrap_or_else(|e| e.into_inner());
    Some(core::mem::transmute::<
        std::sync::MutexGuard<'_, ()>,
        std::sync::MutexGuard<'static, ()>,
    >(g))
}

fn block_size_for(a: &Allocator, heap: u32) -> u64 {
    let hs = a.mem.memory_heaps[heap as usize].size.min(a.heap_limit[heap as usize]);
    if hs > LARGE_HEAP {
        a.block_size
    } else {
        hs.max(64 * 1024).min(a.block_size)
    }
}

pub unsafe fn create(info: *const VmaAllocatorCreateInfo, out: *mut VmaAllocator) -> VkResult {
    if info.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let ci = *info;
    if ci.device.is_null() || ci.physical_device.is_null() || ci.instance.is_null() {
        return VK_ERROR_FEATURE_NOT_PRESENT;
    }
    let provided = if ci.p_vulkan_functions.is_null() {
        None
    } else {
        Some(&*ci.p_vulkan_functions)
    };
    let funcs = match crate::load::resolve(ci.instance, ci.device, provided) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let mut props: VkPhysicalDeviceProperties = core::mem::zeroed();
    let mut mem: VkPhysicalDeviceMemoryProperties = core::mem::zeroed();
    funcs.vk_get_physical_device_properties.unwrap()(ci.physical_device, &mut props);
    funcs.vk_get_physical_device_memory_properties.unwrap()(ci.physical_device, &mut mem);
    let mut heap_limit = [VK_WHOLE_SIZE; VK_MAX_MEMORY_HEAPS];
    if !ci.p_heap_size_limit.is_null() {
        for i in 0..mem.memory_heap_count as usize {
            heap_limit[i] = *ci.p_heap_size_limit.add(i);
            if heap_limit[i] != VK_WHOLE_SIZE {
                mem.memory_heaps[i].size = heap_limit[i].min(mem.memory_heaps[i].size);
            }
        }
    }
    let mut callbacks: VmaDeviceMemoryCallbacks = core::mem::zeroed();
    if !ci.p_device_memory_callbacks.is_null() {
        callbacks = *ci.p_device_memory_callbacks;
    }
    let ntypes = mem.memory_type_count as usize;
    let mut types = Vec::with_capacity(ntypes);
    for _ in 0..ntypes {
        types.push(TypePool {
            blocks: Vec::new(),
            allocations: Vec::new(),
        });
    }
    let a = Box::new(Allocator {
        flags: ci.flags,
        instance: ci.instance,
        physical: ci.physical_device,
        device: ci.device,
        funcs,
        props,
        mem,
        heap_limit,
        block_size: if ci.preferred_large_heap_block_size == 0 {
            DEFAULT_LARGE_BLOCK
        } else {
            ci.preferred_large_heap_block_size
        },
        callbacks,
        types,
        pools: Vec::new(),
        frame: 0,
        lock: Mutex::new(()),
    });
    *out = Box::into_raw(a);
    VK_SUCCESS
}

pub unsafe fn destroy(allocator: VmaAllocator) {
    if allocator.is_null() {
        return;
    }
    let _g = lock_if(allocator);
    let a = &mut *allocator;
    let pools = core::mem::take(&mut a.pools);
    for p in pools {
        destroy_pool_inner(a, p);
    }
    let ntypes = a.types.len();
    for i in 0..ntypes {
        let allocs = core::mem::take(&mut a.types[i].allocations);
        for alloc in allocs {
            let dedicated = (*alloc).dedicated;
            let mapped = (*alloc).mapped;
            let memory = (*alloc).memory;
            let type_index = (*alloc).type_index;
            let size = (*alloc).size;
            if dedicated {
                if !mapped.is_null() {
                    if let Some(f) = a.funcs.vk_unmap_memory {
                        f(a.device, memory);
                    }
                }
                free_vk_memory(a, type_index, memory, size);
            }
            drop(Box::from_raw(alloc));
        }
        let blocks = core::mem::take(&mut a.types[i].blocks);
        for b in blocks {
            free_vk_memory(a, b.type_index, b.memory, b.size);
        }
    }
    drop(_g);
    drop(Box::from_raw(allocator));
}

unsafe fn free_vk_memory(a: &Allocator, ty: u32, memory: VkDeviceMemory, size: u64) {
    if let Some(cb) = a.callbacks.pfn_free {
        cb(a as *const _ as VmaAllocator, ty, memory, size, a.callbacks.p_user_data);
    }
    if let Some(f) = a.funcs.vk_free_memory {
        f(a.device, memory, core::ptr::null());
    }
}

unsafe fn alloc_vk_memory(
    a: &Allocator,
    ty: u32,
    size: u64,
    dedicated_buffer: VkBuffer,
    dedicated_image: VkImage,
) -> Result<VkDeviceMemory, VkResult> {
    let mut flags_info = VkMemoryAllocateFlagsInfo {
        s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_FLAGS_INFO,
        p_next: core::ptr::null(),
        flags: 0,
        device_mask: 0,
    };
    let mut ded = VkMemoryDedicatedAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO,
        p_next: core::ptr::null(),
        image: 0,
        buffer: 0,
    };
    let mut info = VkMemoryAllocateInfo {
        s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        p_next: core::ptr::null(),
        allocation_size: size,
        memory_type_index: ty,
    };
    if a.flags & VMA_ALLOCATOR_CREATE_BUFFER_DEVICE_ADDRESS_BIT != 0 {
        flags_info.flags = VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT;
        flags_info.p_next = info.p_next;
        info.p_next = &flags_info as *const _ as *const c_void;
    }
    if dedicated_buffer != 0 || dedicated_image != 0 {
        ded.buffer = dedicated_buffer;
        ded.image = dedicated_image;
        ded.p_next = info.p_next;
        info.p_next = &ded as *const _ as *const c_void;
    }
    let mut mem = 0u64;
    let r = a.funcs.vk_allocate_memory.unwrap()(a.device, &info, core::ptr::null(), &mut mem);
    if r != VK_SUCCESS {
        return Err(r);
    }
    if let Some(cb) = a.callbacks.pfn_allocate {
        cb(a as *const _ as VmaAllocator, ty, mem, size, a.callbacks.p_user_data);
    }
    Ok(mem)
}

pub fn find_memory_type(
    a: &Allocator,
    memory_type_bits: u32,
    create: &VmaAllocationCreateInfo,
) -> Result<u32, VkResult> {
    if !create.pool.is_null() {
        unsafe {
            return Ok((*create.pool).create.memory_type_index);
        }
    }
    let mut bits = if create.memory_type_bits == 0 {
        u32::MAX
    } else {
        create.memory_type_bits
    } & memory_type_bits;
    let mut required = create.required_flags;
    let mut preferred = create.preferred_flags;
    match create.usage {
        VMA_MEMORY_USAGE_GPU_ONLY => preferred |= VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT,
        VMA_MEMORY_USAGE_CPU_ONLY => {
            required |= VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
        }
        VMA_MEMORY_USAGE_CPU_TO_GPU => {
            required |= VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT;
            preferred |= VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
        }
        VMA_MEMORY_USAGE_GPU_TO_CPU => {
            required |= VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT;
            preferred |= VK_MEMORY_PROPERTY_HOST_CACHED_BIT;
        }
        VMA_MEMORY_USAGE_CPU_COPY => {}
        VMA_MEMORY_USAGE_GPU_LAZILY_ALLOCATED => {
            required |= VK_MEMORY_PROPERTY_LAZILY_ALLOCATED_BIT;
        }
        VMA_MEMORY_USAGE_AUTO | VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE | VMA_MEMORY_USAGE_AUTO_PREFER_HOST => {
            let host = create.flags
                & (VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT
                    | VMA_ALLOCATION_CREATE_HOST_ACCESS_RANDOM_BIT)
                != 0;
            if host {
                required |= VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT;
                if create.flags & VMA_ALLOCATION_CREATE_HOST_ACCESS_RANDOM_BIT != 0 {
                    preferred |= VK_MEMORY_PROPERTY_HOST_CACHED_BIT;
                }
            } else {
                preferred |= VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
            }
            if create.usage == VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE {
                preferred |= VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
            }
        }
        _ => {}
    }
    if a.flags & VMA_ALLOCATOR_CREATE_AMD_DEVICE_COHERENT_MEMORY_BIT == 0 {
        // refuse AMD coherent types unless the flag is set
        for i in 0..a.mem.memory_type_count {
            let f = a.mem.memory_types[i as usize].property_flags;
            if f & VK_MEMORY_PROPERTY_DEVICE_COHERENT_BIT_AMD != 0 {
                bits &= !(1 << i);
            }
        }
    }
    let mut best: Option<(u32, u32)> = None;
    for i in 0..a.mem.memory_type_count {
        if bits & (1 << i) == 0 {
            continue;
        }
        let f = a.mem.memory_types[i as usize].property_flags;
        if f & required != required {
            continue;
        }
        if create.usage == VMA_MEMORY_USAGE_CPU_COPY && f & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT != 0 {
            continue;
        }
        let score = (f & preferred).count_ones();
        match best {
            None => best = Some((score, i)),
            Some((s, _)) if score > s => best = Some((score, i)),
            _ => {}
        }
    }
    best.map(|(_, i)| i).ok_or(VK_ERROR_FEATURE_NOT_PRESENT)
}

unsafe fn want_dedicated(create: &VmaAllocationCreateInfo, req: &VkMemoryRequirements) -> bool {
    if create.flags & VMA_ALLOCATION_CREATE_DEDICATED_MEMORY_BIT != 0 {
        return true;
    }
    if create.usage == VMA_MEMORY_USAGE_GPU_LAZILY_ALLOCATED {
        return true;
    }
    req.size >= 256 * 1024 * 1024
}

unsafe fn place(
    a: &mut Allocator,
    req: &VkMemoryRequirements,
    create: &VmaAllocationCreateInfo,
    extra_align: u64,
    dedicated_buffer: VkBuffer,
    dedicated_image: VkImage,
) -> Result<*mut Allocation, VkResult> {
    let ty = find_memory_type(a, req.memory_type_bits, create)?;
    let mut align = req.alignment.max(1);
    if extra_align > 1 {
        align = align.max(extra_align);
    }
    if !create.pool.is_null() {
        let palign = (*create.pool).create.min_allocation_alignment;
        if palign > 1 {
            align = align.max(palign);
        }
    }
    let size = align_up(req.size.max(1), align);
    let dedicated = want_dedicated(create, req) && create.flags & VMA_ALLOCATION_CREATE_NEVER_ALLOCATE_BIT == 0;
    if dedicated {
        return dedicated_alloc(a, ty, size, create, dedicated_buffer, dedicated_image);
    }
    let heap = a.mem.memory_types[ty as usize].heap_index;
    let bsize = if !create.pool.is_null() && (*create.pool).create.block_size != 0 {
        (*create.pool).create.block_size
    } else {
        block_size_for(a, heap)
    };
    if size > bsize {
        if create.flags & VMA_ALLOCATION_CREATE_NEVER_ALLOCATE_BIT != 0 {
            return Err(VK_ERROR_OUT_OF_DEVICE_MEMORY);
        }
        return dedicated_alloc(a, ty, size, create, dedicated_buffer, dedicated_image);
    }
    let flags = create.flags;
    let try_blocks = |blocks: &mut Vec<Block>| -> Option<(usize, u64)> {
        for (i, b) in blocks.iter_mut().enumerate() {
            if let Some(off) = b.free.alloc(size, align, flags) {
                return Some((i, off));
            }
        }
        None
    };
    let (block_idx, offset) = if !create.pool.is_null() {
        let found = try_blocks(&mut (*create.pool).blocks);
        if let Some(x) = found {
            x
        } else {
            if create.flags & VMA_ALLOCATION_CREATE_NEVER_ALLOCATE_BIT != 0 {
                return Err(VK_ERROR_OUT_OF_DEVICE_MEMORY);
            }
            let (maxb, pflags) = {
                let pool = &*create.pool;
                let maxb = if pool.create.max_block_count == 0 {
                    usize::MAX
                } else {
                    pool.create.max_block_count
                };
                (maxb, pool.create.flags)
            };
            if (*create.pool).blocks.len() >= maxb {
                return Err(VK_ERROR_OUT_OF_DEVICE_MEMORY);
            }
            new_block(a, ty, bsize, pflags, dedicated_buffer, dedicated_image)?;
            let b = a.types[ty as usize].blocks.pop().unwrap();
            let pool = &mut *create.pool;
            pool.blocks.push(b);
            let i = pool.blocks.len() - 1;
            let off = pool.blocks[i]
                .free
                .alloc(size, align, flags)
                .ok_or(VK_ERROR_OUT_OF_DEVICE_MEMORY)?;
            (i, off)
        }
    } else {
        let found = try_blocks(&mut a.types[ty as usize].blocks);
        if let Some(x) = found {
            x
        } else {
            if create.flags & VMA_ALLOCATION_CREATE_NEVER_ALLOCATE_BIT != 0 {
                return Err(VK_ERROR_OUT_OF_DEVICE_MEMORY);
            }
            new_block(a, ty, bsize, 0, 0, 0)?;
            let t = &mut a.types[ty as usize];
            let i = t.blocks.len() - 1;
            let off = t.blocks[i]
                .free
                .alloc(size, align, flags)
                .ok_or(VK_ERROR_OUT_OF_DEVICE_MEMORY)?;
            (i, off)
        }
    };
    let (memory, mapped_base) = if !create.pool.is_null() {
        let b = &(*create.pool).blocks[block_idx];
        (b.memory, b.mapped)
    } else {
        let b = &a.types[ty as usize].blocks[block_idx];
        (b.memory, b.mapped)
    };
    let mut mapped = core::ptr::null_mut();
    if create.flags & VMA_ALLOCATION_CREATE_MAPPED_BIT != 0 {
        mapped = map_block(a, ty, block_idx, create.pool, memory, offset)?;
    }
    let _ = mapped_base;
    let user = if create.flags & VMA_ALLOCATION_CREATE_USER_DATA_COPY_STRING_BIT != 0 {
        core::ptr::null_mut()
    } else {
        create.p_user_data
    };
    let name = if create.flags & VMA_ALLOCATION_CREATE_USER_DATA_COPY_STRING_BIT != 0
        && !create.p_user_data.is_null()
    {
        Some(std::ffi::CStr::from_ptr(create.p_user_data as *const c_char).to_owned())
    } else {
        None
    };
    let alloc = Box::new(Allocation {
        memory,
        offset,
        size,
        type_index: ty,
        dedicated: false,
        block: block_idx,
        pool: create.pool,
        map_refs: if mapped.is_null() { 0 } else { 1 },
        user_data: user,
        name,
        mapped,
    });
    let ptr = Box::into_raw(alloc);
    if !create.pool.is_null() {
        (*create.pool).allocations.push(ptr);
    } else {
        a.types[ty as usize].allocations.push(ptr);
    }
    Ok(ptr)
}

unsafe fn new_block(
    a: &mut Allocator,
    ty: u32,
    size: u64,
    pool_flags: u32,
    buf: VkBuffer,
    img: VkImage,
) -> Result<(), VkResult> {
    let mem = alloc_vk_memory(a, ty, size, buf, img)?;
    let linear = pool_flags & VMA_POOL_CREATE_LINEAR_ALGORITHM_BIT != 0;
    a.types[ty as usize].blocks.push(Block {
        memory: mem,
        size,
        mapped: core::ptr::null_mut(),
        map_refs: 0,
        free: FreeList::new(size, linear),
        type_index: ty,
    });
    Ok(())
}

unsafe fn dedicated_alloc(
    a: &mut Allocator,
    ty: u32,
    size: u64,
    create: &VmaAllocationCreateInfo,
    buf: VkBuffer,
    img: VkImage,
) -> Result<*mut Allocation, VkResult> {
    let can_alias = create.flags & VMA_ALLOCATION_CREATE_CAN_ALIAS_BIT != 0;
    let (db, di) = if can_alias { (0, 0) } else { (buf, img) };
    let mem = alloc_vk_memory(a, ty, size, db, di)?;
    let mut mapped = core::ptr::null_mut();
    if create.flags & VMA_ALLOCATION_CREATE_MAPPED_BIT != 0 {
        let props = a.mem.memory_types[ty as usize].property_flags;
        if props & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT != 0 {
            let r = a.funcs.vk_map_memory.unwrap()(
                a.device,
                mem,
                0,
                VK_WHOLE_SIZE,
                0,
                &mut mapped,
            );
            if r != VK_SUCCESS {
                a.funcs.vk_free_memory.unwrap()(a.device, mem, core::ptr::null());
                return Err(r);
            }
        }
    }
    let user = if create.flags & VMA_ALLOCATION_CREATE_USER_DATA_COPY_STRING_BIT != 0 {
        core::ptr::null_mut()
    } else {
        create.p_user_data
    };
    let name = if create.flags & VMA_ALLOCATION_CREATE_USER_DATA_COPY_STRING_BIT != 0
        && !create.p_user_data.is_null()
    {
        Some(std::ffi::CStr::from_ptr(create.p_user_data as *const c_char).to_owned())
    } else {
        None
    };
    let alloc = Box::new(Allocation {
        memory: mem,
        offset: 0,
        size,
        type_index: ty,
        dedicated: true,
        block: usize::MAX,
        pool: create.pool,
        map_refs: if mapped.is_null() { 0 } else { 1 },
        user_data: user,
        name,
        mapped,
    });
    let ptr = Box::into_raw(alloc);
    a.types[ty as usize].allocations.push(ptr);
    Ok(ptr)
}

unsafe fn map_block(
    a: &mut Allocator,
    ty: u32,
    block_idx: usize,
    pool: VmaPool,
    memory: VkDeviceMemory,
    offset: u64,
) -> Result<*mut c_void, VkResult> {
    let props = a.mem.memory_types[ty as usize].property_flags;
    if props & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT == 0 {
        return Ok(core::ptr::null_mut());
    }
    let block = if !pool.is_null() {
        &mut (*pool).blocks[block_idx]
    } else {
        &mut a.types[ty as usize].blocks[block_idx]
    };
    if block.mapped.is_null() {
        let r = a.funcs.vk_map_memory.unwrap()(
            a.device,
            memory,
            0,
            VK_WHOLE_SIZE,
            0,
            &mut block.mapped,
        );
        if r != VK_SUCCESS {
            return Err(r);
        }
    }
    block.map_refs += 1;
    Ok((block.mapped as usize + offset as usize) as *mut c_void)
}

pub unsafe fn allocate_memory(
    allocator: VmaAllocator,
    req: *const VkMemoryRequirements,
    create: *const VmaAllocationCreateInfo,
    out: *mut VmaAllocation,
    info: *mut VmaAllocationInfo,
) -> VkResult {
    if allocator.is_null() || req.is_null() || create.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    match place(a, &*req, &*create, 1, 0, 0) {
        Ok(p) => {
            *out = p;
            if !info.is_null() {
                fill_info(p, info);
            }
            VK_SUCCESS
        }
        Err(e) => {
            *out = core::ptr::null_mut();
            e
        }
    }
}

pub unsafe fn free_memory(allocator: VmaAllocator, allocation: VmaAllocation) {
    if allocator.is_null() || allocation.is_null() {
        return;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    free_inner(a, allocation);
}

unsafe fn free_inner(a: &mut Allocator, allocation: VmaAllocation) {
    let dedicated = (*allocation).dedicated;
    let mapped = (*allocation).mapped;
    let memory = (*allocation).memory;
    let type_index = (*allocation).type_index;
    let size = (*allocation).size;
    let pool = (*allocation).pool;
    let block = (*allocation).block;
    let map_refs = (*allocation).map_refs;
    let offset = (*allocation).offset;
    if dedicated {
        if !mapped.is_null() {
            a.funcs.vk_unmap_memory.unwrap()(a.device, memory);
        }
        free_vk_memory(a, type_index, memory, size);
        if let Some(t) = a.types.get_mut(type_index as usize) {
            t.allocations.retain(|p| *p != allocation);
        }
    } else if !pool.is_null() {
        let unmap = a.funcs.vk_unmap_memory;
        let device = a.device;
        let pool = &mut *pool;
        if let Some(b) = pool.blocks.get_mut(block) {
            b.free.free(offset, size);
            unmap_if(unmap, device, b, map_refs);
        }
        pool.allocations.retain(|p| *p != allocation);
    } else {
        let unmap = a.funcs.vk_unmap_memory;
        let device = a.device;
        if let Some(t) = a.types.get_mut(type_index as usize) {
            if let Some(b) = t.blocks.get_mut(block) {
                b.free.free(offset, size);
                unmap_if(unmap, device, b, map_refs);
            }
            t.allocations.retain(|p| *p != allocation);
        }
    }
    drop(Box::from_raw(allocation));
}

unsafe fn unmap_if(unmap: PFN_vkUnmapMemory, device: VkDevice, b: &mut Block, refs: u32) {
    if refs == 0 || b.mapped.is_null() {
        return;
    }
    b.map_refs = b.map_refs.saturating_sub(refs);
    if b.map_refs == 0 {
        if let Some(f) = unmap {
            f(device, b.memory);
        }
        b.mapped = core::ptr::null_mut();
    }
}

pub unsafe fn fill_info(p: *mut Allocation, info: *mut VmaAllocationInfo) {
    let a = &*p;
    *info = VmaAllocationInfo {
        memory_type: a.type_index,
        device_memory: a.memory,
        offset: a.offset,
        size: a.size,
        p_mapped_data: a.mapped,
        p_user_data: a.user_data,
        p_name: a.name.as_ref().map(|s| s.as_ptr()).unwrap_or(core::ptr::null()),
    };
}

pub unsafe fn map_memory(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    out: *mut *mut c_void,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    let al = &mut *allocation;
    let props = a.mem.memory_types[al.type_index as usize].property_flags;
    if props & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT == 0 {
        return VK_ERROR_FEATURE_NOT_PRESENT;
    }
    if !al.mapped.is_null() {
        al.map_refs += 1;
        *out = al.mapped;
        return VK_SUCCESS;
    }
    if al.dedicated {
        let mut p = core::ptr::null_mut();
        let r = a.funcs.vk_map_memory.unwrap()(a.device, al.memory, 0, VK_WHOLE_SIZE, 0, &mut p);
        if r != VK_SUCCESS {
            return r;
        }
        al.mapped = p;
        al.map_refs = 1;
        *out = p;
        return VK_SUCCESS;
    }
    match map_block(a, al.type_index, al.block, al.pool, al.memory, al.offset) {
        Ok(p) => {
            al.mapped = p;
            al.map_refs = 1;
            *out = p;
            VK_SUCCESS
        }
        Err(e) => e,
    }
}

pub unsafe fn unmap_memory(allocator: VmaAllocator, allocation: VmaAllocation) {
    if allocator.is_null() || allocation.is_null() {
        return;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    let al = &mut *allocation;
    if al.map_refs == 0 {
        return;
    }
    al.map_refs -= 1;
    if al.map_refs != 0 {
        return;
    }
    if al.dedicated {
        a.funcs.vk_unmap_memory.unwrap()(a.device, al.memory);
        al.mapped = core::ptr::null_mut();
        return;
    }
    let unmap = a.funcs.vk_unmap_memory;
    let device = a.device;
    let b = if !al.pool.is_null() {
        (*al.pool).blocks.get_mut(al.block)
    } else {
        a.types[al.type_index as usize].blocks.get_mut(al.block)
    };
    if let Some(b) = b {
        unmap_if(unmap, device, b, 1);
    }
    al.mapped = core::ptr::null_mut();
}

unsafe fn flush_range(
    a: &Allocator,
    memory: VkDeviceMemory,
    offset: u64,
    size: u64,
    invalidate: bool,
) -> VkResult {
    let atom = a.props.limits.non_coherent_atom_size.max(1);
    let off = offset - (offset % atom);
    let end = if size == VK_WHOLE_SIZE {
        VK_WHOLE_SIZE
    } else {
        align_up(offset + size, atom)
    };
    let range = VkMappedMemoryRange {
        s_type: VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE,
        p_next: core::ptr::null(),
        memory,
        offset: off,
        size: if end == VK_WHOLE_SIZE {
            VK_WHOLE_SIZE
        } else {
            end - off
        },
    };
    if invalidate {
        a.funcs.vk_invalidate_mapped_memory_ranges.unwrap()(a.device, 1, &range)
    } else {
        a.funcs.vk_flush_mapped_memory_ranges.unwrap()(a.device, 1, &range)
    }
}

pub unsafe fn flush_allocation(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    offset: VkDeviceSize,
    size: VkDeviceSize,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() {
        return VK_SUCCESS;
    }
    let a = &*allocator;
    let al = &*allocation;
    flush_range(a, al.memory, al.offset + offset, size, false)
}

pub unsafe fn invalidate_allocation(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    offset: VkDeviceSize,
    size: VkDeviceSize,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() {
        return VK_SUCCESS;
    }
    let a = &*allocator;
    let al = &*allocation;
    flush_range(a, al.memory, al.offset + offset, size, true)
}

pub unsafe fn create_buffer(
    allocator: VmaAllocator,
    buf_ci: *const VkBufferCreateInfo,
    alloc_ci: *const VmaAllocationCreateInfo,
    extra_align: u64,
    out_buf: *mut VkBuffer,
    out_alloc: *mut VmaAllocation,
    info: *mut VmaAllocationInfo,
) -> VkResult {
    if allocator.is_null() || buf_ci.is_null() || alloc_ci.is_null() || out_buf.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    let mut buf = 0u64;
    let r = a.funcs.vk_create_buffer.unwrap()(a.device, buf_ci, core::ptr::null(), &mut buf);
    if r != VK_SUCCESS {
        return r;
    }
    let mut req = VkMemoryRequirements {
        size: 0,
        alignment: 1,
        memory_type_bits: 0,
    };
    a.funcs.vk_get_buffer_memory_requirements.unwrap()(a.device, buf, &mut req);
    if extra_align > 1 {
        req.alignment = req.alignment.max(extra_align);
    }
    match place(a, &req, &*alloc_ci, extra_align, buf, 0) {
        Ok(p) => {
            if (*alloc_ci).flags & VMA_ALLOCATION_CREATE_DONT_BIND_BIT == 0 {
                let br = a.funcs.vk_bind_buffer_memory.unwrap()(
                    a.device,
                    buf,
                    (*p).memory,
                    (*p).offset,
                );
                if br != VK_SUCCESS {
                    free_inner(a, p);
                    a.funcs.vk_destroy_buffer.unwrap()(a.device, buf, core::ptr::null());
                    return br;
                }
            }
            *out_buf = buf;
            if !out_alloc.is_null() {
                *out_alloc = p;
            }
            if !info.is_null() {
                fill_info(p, info);
            }
            VK_SUCCESS
        }
        Err(e) => {
            a.funcs.vk_destroy_buffer.unwrap()(a.device, buf, core::ptr::null());
            e
        }
    }
}

pub unsafe fn create_image(
    allocator: VmaAllocator,
    img_ci: *const VkImageCreateInfo,
    alloc_ci: *const VmaAllocationCreateInfo,
    out_img: *mut VkImage,
    out_alloc: *mut VmaAllocation,
    info: *mut VmaAllocationInfo,
) -> VkResult {
    if allocator.is_null() || img_ci.is_null() || alloc_ci.is_null() || out_img.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    let mut img = 0u64;
    let r = a.funcs.vk_create_image.unwrap()(a.device, img_ci, core::ptr::null(), &mut img);
    if r != VK_SUCCESS {
        return r;
    }
    let mut req = VkMemoryRequirements {
        size: 0,
        alignment: 1,
        memory_type_bits: 0,
    };
    a.funcs.vk_get_image_memory_requirements.unwrap()(a.device, img, &mut req);
    match place(a, &req, &*alloc_ci, 1, 0, img) {
        Ok(p) => {
            if (*alloc_ci).flags & VMA_ALLOCATION_CREATE_DONT_BIND_BIT == 0 {
                let br =
                    a.funcs.vk_bind_image_memory.unwrap()(a.device, img, (*p).memory, (*p).offset);
                if br != VK_SUCCESS {
                    free_inner(a, p);
                    a.funcs.vk_destroy_image.unwrap()(a.device, img, core::ptr::null());
                    return br;
                }
            }
            *out_img = img;
            if !out_alloc.is_null() {
                *out_alloc = p;
            }
            if !info.is_null() {
                fill_info(p, info);
            }
            VK_SUCCESS
        }
        Err(e) => {
            a.funcs.vk_destroy_image.unwrap()(a.device, img, core::ptr::null());
            e
        }
    }
}

pub unsafe fn destroy_buffer(allocator: VmaAllocator, buffer: VkBuffer, allocation: VmaAllocation) {
    if allocator.is_null() {
        return;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    if buffer != 0 {
        a.funcs.vk_destroy_buffer.unwrap()(a.device, buffer, core::ptr::null());
    }
    if !allocation.is_null() {
        free_inner(a, allocation);
    }
}

pub unsafe fn destroy_image(allocator: VmaAllocator, image: VkImage, allocation: VmaAllocation) {
    if allocator.is_null() {
        return;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    if image != 0 {
        a.funcs.vk_destroy_image.unwrap()(a.device, image, core::ptr::null());
    }
    if !allocation.is_null() {
        free_inner(a, allocation);
    }
}

pub unsafe fn create_pool(
    allocator: VmaAllocator,
    info: *const VmaPoolCreateInfo,
    out: *mut VmaPool,
) -> VkResult {
    if allocator.is_null() || info.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    let ci = *info;
    if ci.memory_type_index >= a.mem.memory_type_count {
        return VK_ERROR_FEATURE_NOT_PRESENT;
    }
    let p = Box::new(Pool {
        create: ci,
        name: None,
        blocks: Vec::new(),
        allocations: Vec::new(),
    });
    let ptr = Box::into_raw(p);
    for _ in 0..ci.min_block_count {
        let heap = a.mem.memory_types[ci.memory_type_index as usize].heap_index;
        let bsize = if ci.block_size != 0 {
            ci.block_size
        } else {
            block_size_for(a, heap)
        };
        if new_block(a, ci.memory_type_index, bsize, ci.flags, 0, 0).is_err() {
            break;
        }
        let b = a.types[ci.memory_type_index as usize].blocks.pop().unwrap();
        (*ptr).blocks.push(b);
    }
    a.pools.push(ptr);
    *out = ptr;
    VK_SUCCESS
}

pub unsafe fn destroy_pool(allocator: VmaAllocator, pool: VmaPool) {
    if allocator.is_null() || pool.is_null() {
        return;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    a.pools.retain(|p| *p != pool);
    destroy_pool_inner(a, pool);
}

unsafe fn destroy_pool_inner(a: &mut Allocator, pool: VmaPool) {
    let p = &mut *pool;
    for al in p.allocations.drain(..) {
        drop(Box::from_raw(al));
    }
    for b in p.blocks.drain(..) {
        free_vk_memory(a, b.type_index, b.memory, b.size);
    }
    drop(Box::from_raw(pool));
}

pub unsafe fn bind_buffer(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    buffer: VkBuffer,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() {
        return VK_ERROR_UNKNOWN;
    }
    let a = &*allocator;
    a.funcs.vk_bind_buffer_memory.unwrap()(a.device, buffer, (*allocation).memory, (*allocation).offset)
}

pub unsafe fn bind_image(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    image: VkImage,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() {
        return VK_ERROR_UNKNOWN;
    }
    let a = &*allocator;
    a.funcs.vk_bind_image_memory.unwrap()(a.device, image, (*allocation).memory, (*allocation).offset)
}

pub fn add_stats(dst: &mut VmaStatistics, src: &VmaStatistics) {
    dst.block_count += src.block_count;
    dst.allocation_count += src.allocation_count;
    dst.block_bytes += src.block_bytes;
    dst.allocation_bytes += src.allocation_bytes;
}

pub fn type_stats(a: &Allocator, ty: usize) -> VmaStatistics {
    let t = &a.types[ty];
    VmaStatistics {
        block_count: t.blocks.len() as u32,
        allocation_count: t.allocations.len() as u32,
        block_bytes: t.blocks.iter().map(|b| b.size).sum(),
        allocation_bytes: t.allocations.iter().map(|p| unsafe { (**p).size }).sum(),
    }
}

pub unsafe fn aliasing_buffer(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    offset: VkDeviceSize,
    buf_ci: *const VkBufferCreateInfo,
    out: *mut VkBuffer,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() || buf_ci.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &*allocator;
    let mut buf = 0u64;
    let r = a.funcs.vk_create_buffer.unwrap()(a.device, buf_ci, core::ptr::null(), &mut buf);
    if r != VK_SUCCESS {
        return r;
    }
    let r = a.funcs.vk_bind_buffer_memory.unwrap()(
        a.device,
        buf,
        (*allocation).memory,
        (*allocation).offset + offset,
    );
    if r != VK_SUCCESS {
        a.funcs.vk_destroy_buffer.unwrap()(a.device, buf, core::ptr::null());
        return r;
    }
    *out = buf;
    VK_SUCCESS
}

pub unsafe fn aliasing_image(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    offset: VkDeviceSize,
    img_ci: *const VkImageCreateInfo,
    out: *mut VkImage,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() || img_ci.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &*allocator;
    let mut img = 0u64;
    let r = a.funcs.vk_create_image.unwrap()(a.device, img_ci, core::ptr::null(), &mut img);
    if r != VK_SUCCESS {
        return r;
    }
    let r = a.funcs.vk_bind_image_memory.unwrap()(
        a.device,
        img,
        (*allocation).memory,
        (*allocation).offset + offset,
    );
    if r != VK_SUCCESS {
        a.funcs.vk_destroy_image.unwrap()(a.device, img, core::ptr::null());
        return r;
    }
    *out = img;
    VK_SUCCESS
}

pub unsafe fn copy_to_allocation(
    allocator: VmaAllocator,
    src: *const c_void,
    allocation: VmaAllocation,
    offset: VkDeviceSize,
    size: VkDeviceSize,
) -> VkResult {
    if src.is_null() || allocation.is_null() {
        return VK_ERROR_UNKNOWN;
    }
    let mut p = core::ptr::null_mut();
    let r = map_memory(allocator, allocation, &mut p);
    if r != VK_SUCCESS {
        return r;
    }
    core::ptr::copy_nonoverlapping(src as *const u8, (p as *mut u8).add(offset as usize), size as usize);
    let _ = flush_allocation(allocator, allocation, offset, size);
    unmap_memory(allocator, allocation);
    VK_SUCCESS
}

pub unsafe fn copy_from_allocation(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    offset: VkDeviceSize,
    dst: *mut c_void,
    size: VkDeviceSize,
) -> VkResult {
    if dst.is_null() || allocation.is_null() {
        return VK_ERROR_UNKNOWN;
    }
    let mut p = core::ptr::null_mut();
    let r = map_memory(allocator, allocation, &mut p);
    if r != VK_SUCCESS {
        return r;
    }
    let _ = invalidate_allocation(allocator, allocation, offset, size);
    core::ptr::copy_nonoverlapping((p as *const u8).add(offset as usize), dst as *mut u8, size as usize);
    unmap_memory(allocator, allocation);
    VK_SUCCESS
}

pub unsafe fn dummy_buffer_reqs(
    a: &Allocator,
    ci: *const VkBufferCreateInfo,
) -> Result<VkMemoryRequirements, VkResult> {
    if let Some(f) = a.funcs.vk_get_device_buffer_memory_requirements {
        let info = VkDeviceBufferMemoryRequirements {
            s_type: VK_STRUCTURE_TYPE_DEVICE_BUFFER_MEMORY_REQUIREMENTS,
            p_next: core::ptr::null(),
            p_create_info: ci,
        };
        let mut out = VkMemoryRequirements2 {
            s_type: VK_STRUCTURE_TYPE_MEMORY_REQUIREMENTS_2,
            p_next: core::ptr::null_mut(),
            memory_requirements: VkMemoryRequirements {
                size: 0,
                alignment: 1,
                memory_type_bits: 0,
            },
        };
        f(a.device, &info, &mut out);
        return Ok(out.memory_requirements);
    }
    let mut buf = 0u64;
    let r = a.funcs.vk_create_buffer.unwrap()(a.device, ci, core::ptr::null(), &mut buf);
    if r != VK_SUCCESS {
        return Err(r);
    }
    let mut req = VkMemoryRequirements {
        size: 0,
        alignment: 1,
        memory_type_bits: 0,
    };
    a.funcs.vk_get_buffer_memory_requirements.unwrap()(a.device, buf, &mut req);
    a.funcs.vk_destroy_buffer.unwrap()(a.device, buf, core::ptr::null());
    Ok(req)
}

pub unsafe fn dummy_image_reqs(
    a: &Allocator,
    ci: *const VkImageCreateInfo,
) -> Result<VkMemoryRequirements, VkResult> {
    let mut img = 0u64;
    let r = a.funcs.vk_create_image.unwrap()(a.device, ci, core::ptr::null(), &mut img);
    if r != VK_SUCCESS {
        return Err(r);
    }
    let mut req = VkMemoryRequirements {
        size: 0,
        alignment: 1,
        memory_type_bits: 0,
    };
    a.funcs.vk_get_image_memory_requirements.unwrap()(a.device, img, &mut req);
    a.funcs.vk_destroy_image.unwrap()(a.device, img, core::ptr::null());
    Ok(req)
}

/// Trivial defrag: report no moves. Full compacting is a follow-up; ABI is present.
pub unsafe fn begin_defrag(
    allocator: VmaAllocator,
    _info: *const VmaDefragmentationInfo,
    ctx: *mut VmaDefragmentationContext,
) -> VkResult {
    if allocator.is_null() || ctx.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    *ctx = Box::into_raw(Box::new(Defrag {
        moves: Vec::new(),
        stats: VmaDefragmentationStats::default(),
    }));
    VK_SUCCESS
}

pub unsafe fn end_defrag(
    _allocator: VmaAllocator,
    ctx: VmaDefragmentationContext,
    stats: *mut VmaDefragmentationStats,
) {
    if ctx.is_null() {
        return;
    }
    if !stats.is_null() {
        *stats = (*ctx).stats;
    }
    drop(Box::from_raw(ctx));
}

pub unsafe fn begin_defrag_pass(
    _allocator: VmaAllocator,
    ctx: VmaDefragmentationContext,
    pass: *mut VmaDefragmentationPassMoveInfo,
) -> VkResult {
    if ctx.is_null() || pass.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let d = &mut *ctx;
    if d.moves.is_empty() {
        *pass = VmaDefragmentationPassMoveInfo {
            move_count: 0,
            p_moves: core::ptr::null_mut(),
        };
        VK_SUCCESS
    } else {
        *pass = VmaDefragmentationPassMoveInfo {
            move_count: d.moves.len() as u32,
            p_moves: d.moves.as_mut_ptr(),
        };
        VK_INCOMPLETE
    }
}

pub unsafe fn end_defrag_pass(
    _allocator: VmaAllocator,
    ctx: VmaDefragmentationContext,
    _pass: *mut VmaDefragmentationPassMoveInfo,
) -> VkResult {
    if ctx.is_null() {
        return VK_SUCCESS;
    }
    (*ctx).moves.clear();
    VK_SUCCESS
}

pub unsafe fn get_allocator_info(allocator: VmaAllocator, info: *mut VmaAllocatorInfo) {
    if allocator.is_null() || info.is_null() {
        return;
    }
    let a = &*allocator;
    *info = VmaAllocatorInfo {
        instance: a.instance,
        physical_device: a.physical,
        device: a.device,
    };
}

pub unsafe fn get_physical_device_properties(
    allocator: VmaAllocator,
    out: *mut *const VkPhysicalDeviceProperties,
) {
    if allocator.is_null() || out.is_null() {
        return;
    }
    *out = &(*allocator).props;
}

pub unsafe fn get_memory_properties(
    allocator: VmaAllocator,
    out: *mut *const VkPhysicalDeviceMemoryProperties,
) {
    if allocator.is_null() || out.is_null() {
        return;
    }
    *out = &(*allocator).mem;
}

pub unsafe fn get_memory_type_properties(allocator: VmaAllocator, index: u32, flags: *mut VkFlags) {
    if allocator.is_null() || flags.is_null() {
        return;
    }
    let a = &*allocator;
    if index >= a.mem.memory_type_count {
        *flags = 0;
        return;
    }
    *flags = a.mem.memory_types[index as usize].property_flags;
}

pub unsafe fn set_current_frame_index(allocator: VmaAllocator, frame: u32) {
    if allocator.is_null() {
        return;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    a.frame = frame;
}

fn add_detailed(dst: &mut VmaDetailedStatistics, src: &VmaDetailedStatistics) {
    add_stats(&mut dst.statistics, &src.statistics);
    dst.unused_range_count += src.unused_range_count;
    dst.allocation_size_min = dst.allocation_size_min.min(src.allocation_size_min);
    dst.allocation_size_max = dst.allocation_size_max.max(src.allocation_size_max);
    dst.unused_range_size_min = dst.unused_range_size_min.min(src.unused_range_size_min);
    dst.unused_range_size_max = dst.unused_range_size_max.max(src.unused_range_size_max);
}

fn block_detailed(b: &Block, allocs: &[*mut Allocation]) -> VmaDetailedStatistics {
    let mut d = VmaDetailedStatistics::default();
    d.statistics.block_count = 1;
    d.statistics.block_bytes = b.size;
    d.unused_range_count = b.free.unused_range_count();
    for p in allocs {
        let a = unsafe { &**p };
        if a.memory != b.memory {
            continue;
        }
        d.statistics.allocation_count += 1;
        d.statistics.allocation_bytes += a.size;
        d.allocation_size_min = d.allocation_size_min.min(a.size);
        d.allocation_size_max = d.allocation_size_max.max(a.size);
    }
    d
}

fn pool_detailed(p: &Pool) -> VmaDetailedStatistics {
    let mut d = VmaDetailedStatistics::default();
    for b in &p.blocks {
        add_detailed(&mut d, &block_detailed(b, &p.allocations));
    }
    for al in &p.allocations {
        let a = unsafe { &**al };
        if a.dedicated {
            d.statistics.allocation_count += 1;
            d.statistics.allocation_bytes += a.size;
            d.statistics.block_count += 1;
            d.statistics.block_bytes += a.size;
        }
    }
    d
}

pub unsafe fn calculate_statistics(allocator: VmaAllocator, out: *mut VmaTotalStatistics) {
    if allocator.is_null() || out.is_null() {
        return;
    }
    let a = &*allocator;
    let _g = lock_if(allocator);
    let mut total = VmaTotalStatistics {
        memory_type: [VmaDetailedStatistics::default(); VK_MAX_MEMORY_TYPES],
        memory_heap: [VmaDetailedStatistics::default(); VK_MAX_MEMORY_HEAPS],
        total: VmaDetailedStatistics::default(),
    };
    for i in 0..a.types.len() {
        let mut d = VmaDetailedStatistics::default();
        for b in &a.types[i].blocks {
            add_detailed(&mut d, &block_detailed(b, &a.types[i].allocations));
        }
        for al in &a.types[i].allocations {
            let al = &**al;
            if al.dedicated {
                d.statistics.allocation_count += 1;
                d.statistics.allocation_bytes += al.size;
                d.statistics.block_count += 1;
                d.statistics.block_bytes += al.size;
                d.allocation_size_min = d.allocation_size_min.min(al.size);
                d.allocation_size_max = d.allocation_size_max.max(al.size);
            }
        }
        total.memory_type[i] = d;
        let heap = a.mem.memory_types[i].heap_index as usize;
        add_detailed(&mut total.memory_heap[heap], &d);
        add_detailed(&mut total.total, &d);
    }
    for p in &a.pools {
        let d = pool_detailed(&**p);
        let ty = (**p).create.memory_type_index as usize;
        add_detailed(&mut total.memory_type[ty], &d);
        let heap = a.mem.memory_types[ty].heap_index as usize;
        add_detailed(&mut total.memory_heap[heap], &d);
        add_detailed(&mut total.total, &d);
    }
    *out = total;
}

pub unsafe fn get_heap_budgets(allocator: VmaAllocator, budgets: *mut VmaBudget) {
    if allocator.is_null() || budgets.is_null() {
        return;
    }
    let a = &*allocator;
    let _g = lock_if(allocator);
    let n = a.mem.memory_heap_count as usize;
    for i in 0..n {
        let mut st = VmaStatistics::default();
        for (ti, _) in a.types.iter().enumerate() {
            if a.mem.memory_types[ti].heap_index as usize != i {
                continue;
            }
            add_stats(&mut st, &type_stats(a, ti));
        }
        *budgets.add(i) = VmaBudget {
            statistics: st,
            usage: st.block_bytes,
            budget: a.mem.memory_heaps[i].size.min(a.heap_limit[i]),
        };
    }
}

pub unsafe fn find_memory_type_index(
    allocator: VmaAllocator,
    bits: u32,
    create: *const VmaAllocationCreateInfo,
    out: *mut u32,
) -> VkResult {
    if allocator.is_null() || create.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    match find_memory_type(&*allocator, bits, &*create) {
        Ok(i) => {
            *out = i;
            VK_SUCCESS
        }
        Err(e) => e,
    }
}

pub unsafe fn find_memory_type_index_for_buffer_info(
    allocator: VmaAllocator,
    buf_ci: *const VkBufferCreateInfo,
    create: *const VmaAllocationCreateInfo,
    out: *mut u32,
) -> VkResult {
    if allocator.is_null() || buf_ci.is_null() || create.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &*allocator;
    let req = match dummy_buffer_reqs(a, buf_ci) {
        Ok(r) => r,
        Err(e) => return e,
    };
    find_memory_type_index(allocator, req.memory_type_bits, create, out)
}

pub unsafe fn find_memory_type_index_for_image_info(
    allocator: VmaAllocator,
    img_ci: *const VkImageCreateInfo,
    create: *const VmaAllocationCreateInfo,
    out: *mut u32,
) -> VkResult {
    if allocator.is_null() || img_ci.is_null() || create.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &*allocator;
    let req = match dummy_image_reqs(a, img_ci) {
        Ok(r) => r,
        Err(e) => return e,
    };
    find_memory_type_index(allocator, req.memory_type_bits, create, out)
}

pub unsafe fn get_pool_statistics(allocator: VmaAllocator, pool: VmaPool, out: *mut VmaStatistics) {
    if allocator.is_null() || pool.is_null() || out.is_null() {
        return;
    }
    let _g = lock_if(allocator);
    let p = &*pool;
    *out = VmaStatistics {
        block_count: p.blocks.len() as u32,
        allocation_count: p.allocations.len() as u32,
        block_bytes: p.blocks.iter().map(|b| b.size).sum(),
        allocation_bytes: p.allocations.iter().map(|p| (**p).size).sum(),
    };
}

pub unsafe fn calculate_pool_statistics(
    allocator: VmaAllocator,
    pool: VmaPool,
    out: *mut VmaDetailedStatistics,
) {
    if allocator.is_null() || pool.is_null() || out.is_null() {
        return;
    }
    let _g = lock_if(allocator);
    *out = pool_detailed(&*pool);
}

pub unsafe fn check_pool_corruption(_allocator: VmaAllocator, _pool: VmaPool) -> VkResult {
    VK_ERROR_FEATURE_NOT_PRESENT
}

pub unsafe fn get_pool_name(allocator: VmaAllocator, pool: VmaPool, out: *mut *const c_char) {
    if allocator.is_null() || pool.is_null() || out.is_null() {
        return;
    }
    *out = (*pool)
        .name
        .as_ref()
        .map(|s| s.as_ptr())
        .unwrap_or(core::ptr::null());
}

pub unsafe fn set_pool_name(allocator: VmaAllocator, pool: VmaPool, name: *const c_char) {
    if allocator.is_null() || pool.is_null() {
        return;
    }
    let _g = lock_if(allocator);
    (*pool).name = if name.is_null() {
        None
    } else {
        Some(std::ffi::CStr::from_ptr(name).to_owned())
    };
}

pub unsafe fn allocate_memory_pages(
    allocator: VmaAllocator,
    reqs: *const VkMemoryRequirements,
    creates: *const VmaAllocationCreateInfo,
    count: usize,
    out: *mut VmaAllocation,
    infos: *mut VmaAllocationInfo,
) -> VkResult {
    if count == 0 {
        return VK_SUCCESS;
    }
    if allocator.is_null() || reqs.is_null() || creates.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    for i in 0..count {
        *out.add(i) = core::ptr::null_mut();
    }
    for i in 0..count {
        let info = if infos.is_null() {
            core::ptr::null_mut()
        } else {
            infos.add(i)
        };
        let r = allocate_memory(allocator, reqs.add(i), creates.add(i), out.add(i), info);
        if r != VK_SUCCESS {
            for j in 0..i {
                free_memory(allocator, *out.add(j));
            }
            for j in 0..count {
                *out.add(j) = core::ptr::null_mut();
            }
            return r;
        }
    }
    VK_SUCCESS
}

pub unsafe fn allocate_memory_for_buffer(
    allocator: VmaAllocator,
    buffer: VkBuffer,
    create: *const VmaAllocationCreateInfo,
    out: *mut VmaAllocation,
    info: *mut VmaAllocationInfo,
) -> VkResult {
    if allocator.is_null() || create.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    let mut req = VkMemoryRequirements {
        size: 0,
        alignment: 1,
        memory_type_bits: 0,
    };
    a.funcs.vk_get_buffer_memory_requirements.unwrap()(a.device, buffer, &mut req);
    match place(a, &req, &*create, 1, buffer, 0) {
        Ok(p) => {
            *out = p;
            if !info.is_null() {
                fill_info(p, info);
            }
            VK_SUCCESS
        }
        Err(e) => e,
    }
}

pub unsafe fn allocate_memory_for_image(
    allocator: VmaAllocator,
    image: VkImage,
    create: *const VmaAllocationCreateInfo,
    out: *mut VmaAllocation,
    info: *mut VmaAllocationInfo,
) -> VkResult {
    if allocator.is_null() || create.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let a = &mut *allocator;
    let _g = lock_if(a);
    let mut req = VkMemoryRequirements {
        size: 0,
        alignment: 1,
        memory_type_bits: 0,
    };
    a.funcs.vk_get_image_memory_requirements.unwrap()(a.device, image, &mut req);
    match place(a, &req, &*create, 1, 0, image) {
        Ok(p) => {
            *out = p;
            if !info.is_null() {
                fill_info(p, info);
            }
            VK_SUCCESS
        }
        Err(e) => e,
    }
}

pub unsafe fn free_memory_pages(
    allocator: VmaAllocator,
    count: usize,
    allocs: *const VmaAllocation,
) {
    if allocator.is_null() || allocs.is_null() {
        return;
    }
    for i in 0..count {
        free_memory(allocator, *allocs.add(i));
    }
}

pub unsafe fn get_allocation_info(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    info: *mut VmaAllocationInfo,
) {
    if allocator.is_null() || allocation.is_null() || info.is_null() {
        return;
    }
    fill_info(allocation, info);
}

pub unsafe fn get_allocation_info2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    info: *mut VmaAllocationInfo2,
) {
    if allocator.is_null() || allocation.is_null() || info.is_null() {
        return;
    }
    let al = &*allocation;
    fill_info(allocation, &mut (*info).allocation_info);
    (*info).dedicated_memory = if al.dedicated { VK_TRUE } else { VK_FALSE };
    (*info).block_size = if al.dedicated {
        al.size
    } else if !al.pool.is_null() {
        (*al.pool).blocks.get(al.block).map(|b| b.size).unwrap_or(al.size)
    } else {
        (*allocator).types[al.type_index as usize]
            .blocks
            .get(al.block)
            .map(|b| b.size)
            .unwrap_or(al.size)
    };
}

pub unsafe fn set_allocation_user_data(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    user: *mut c_void,
) {
    if allocator.is_null() || allocation.is_null() {
        return;
    }
    (*allocation).user_data = user;
}

pub unsafe fn set_allocation_name(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    name: *const c_char,
) {
    if allocator.is_null() || allocation.is_null() {
        return;
    }
    (*allocation).name = if name.is_null() {
        None
    } else {
        Some(std::ffi::CStr::from_ptr(name).to_owned())
    };
}

pub unsafe fn get_allocation_memory_properties(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    flags: *mut VkFlags,
) {
    if allocator.is_null() || allocation.is_null() || flags.is_null() {
        return;
    }
    let ty = (*allocation).type_index;
    get_memory_type_properties(allocator, ty, flags);
}

pub unsafe fn get_memory_win32_handle(
    _allocator: VmaAllocator,
    _allocation: VmaAllocation,
    _process: *mut c_void,
    _handle: *mut *mut c_void,
) -> VkResult {
    VK_ERROR_FEATURE_NOT_PRESENT
}

pub unsafe fn flush_allocations(
    allocator: VmaAllocator,
    count: u32,
    allocs: *const VmaAllocation,
    offsets: *const VkDeviceSize,
    sizes: *const VkDeviceSize,
) -> VkResult {
    let mut last = VK_SUCCESS;
    for i in 0..count as usize {
        if allocs.is_null() {
            break;
        }
        let al = *allocs.add(i);
        if al.is_null() {
            continue;
        }
        let off = if offsets.is_null() { 0 } else { *offsets.add(i) };
        let sz = if sizes.is_null() {
            VK_WHOLE_SIZE
        } else {
            *sizes.add(i)
        };
        let r = flush_allocation(allocator, al, off, sz);
        if r != VK_SUCCESS {
            last = r;
        }
    }
    last
}

pub unsafe fn invalidate_allocations(
    allocator: VmaAllocator,
    count: u32,
    allocs: *const VmaAllocation,
    offsets: *const VkDeviceSize,
    sizes: *const VkDeviceSize,
) -> VkResult {
    let mut last = VK_SUCCESS;
    for i in 0..count as usize {
        if allocs.is_null() {
            break;
        }
        let al = *allocs.add(i);
        if al.is_null() {
            continue;
        }
        let off = if offsets.is_null() { 0 } else { *offsets.add(i) };
        let sz = if sizes.is_null() {
            VK_WHOLE_SIZE
        } else {
            *sizes.add(i)
        };
        let r = invalidate_allocation(allocator, al, off, sz);
        if r != VK_SUCCESS {
            last = r;
        }
    }
    last
}

pub unsafe fn check_corruption(_allocator: VmaAllocator, _bits: u32) -> VkResult {
    VK_ERROR_FEATURE_NOT_PRESENT
}

pub unsafe fn bind_buffer2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    local_offset: VkDeviceSize,
    buffer: VkBuffer,
    p_next: *const c_void,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() {
        return VK_ERROR_UNKNOWN;
    }
    let a = &*allocator;
    let mem = (*allocation).memory;
    let off = (*allocation).offset + local_offset;
    if !p_next.is_null() || a.funcs.vk_bind_buffer_memory2_khr.is_some() {
        let Some(f) = a.funcs.vk_bind_buffer_memory2_khr else {
            return VK_ERROR_FEATURE_NOT_PRESENT;
        };
        let info = VkBindBufferMemoryInfo {
            s_type: VK_STRUCTURE_TYPE_BIND_BUFFER_MEMORY_INFO,
            p_next,
            buffer,
            memory: mem,
            memory_offset: off,
        };
        return f(a.device, 1, &info);
    }
    a.funcs.vk_bind_buffer_memory.unwrap()(a.device, buffer, mem, off)
}

pub unsafe fn bind_image2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    local_offset: VkDeviceSize,
    image: VkImage,
    p_next: *const c_void,
) -> VkResult {
    if allocator.is_null() || allocation.is_null() {
        return VK_ERROR_UNKNOWN;
    }
    let a = &*allocator;
    let mem = (*allocation).memory;
    let off = (*allocation).offset + local_offset;
    if !p_next.is_null() || a.funcs.vk_bind_image_memory2_khr.is_some() {
        let Some(f) = a.funcs.vk_bind_image_memory2_khr else {
            return VK_ERROR_FEATURE_NOT_PRESENT;
        };
        let info = VkBindImageMemoryInfo {
            s_type: VK_STRUCTURE_TYPE_BIND_IMAGE_MEMORY_INFO,
            p_next,
            image,
            memory: mem,
            memory_offset: off,
        };
        return f(a.device, 1, &info);
    }
    a.funcs.vk_bind_image_memory.unwrap()(a.device, image, mem, off)
}

pub unsafe fn build_stats_string(
    allocator: VmaAllocator,
    out: *mut *mut c_char,
    _detailed: VkBool32,
) {
    if allocator.is_null() || out.is_null() {
        return;
    }
    let mut st: VmaTotalStatistics = core::mem::zeroed();
    calculate_statistics(allocator, &mut st);
    let json = format!(
        "{{\"total\":{{\"blockCount\":{},\"allocationCount\":{},\"blockBytes\":{},\"allocationBytes\":{}}}}}",
        st.total.statistics.block_count,
        st.total.statistics.allocation_count,
        st.total.statistics.block_bytes,
        st.total.statistics.allocation_bytes
    );
    *out = std::ffi::CString::new(json)
        .ok()
        .map(|c| c.into_raw())
        .unwrap_or(core::ptr::null_mut());
}

pub unsafe fn free_stats_string(_allocator: VmaAllocator, s: *mut c_char) {
    crate::virtual_block::free_string(s);
}

pub unsafe fn import_vulkan_functions_from_volk(
    info: *const VmaAllocatorCreateInfo,
    dst: *mut VmaVulkanFunctions,
) -> VkResult {
    if info.is_null() || dst.is_null() {
        return VK_ERROR_OUT_OF_HOST_MEMORY;
    }
    let ci = *info;
    let provided = if ci.p_vulkan_functions.is_null() {
        None
    } else {
        Some(&*ci.p_vulkan_functions)
    };
    match crate::load::resolve(ci.instance, ci.device, provided) {
        Ok(f) => {
            *dst = f;
            VK_SUCCESS
        }
        Err(e) => e,
    }
}
