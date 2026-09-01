//! In-process fake Vulkan for allocator tests (no GPU).
//!
//! Workloads follow Blender GHOST (`GHOST_DeviceVK::init_memory_allocator`)
//! and OpenXR staging: Vulkan 1.2, buffer device address, optional
//! `EXT_MEMORY_PRIORITY` / `KHR_MAINTENANCE4` / `EXT_MEMORY_BUDGET`, many
//! mapped sequential-write vertex/index/uniform buffers, GPU-only images,
//! pools, stats, and VMA 3.4 dedicated / `minAlignment` paths.

use crate::device;
use crate::types::*;
use crate::vk::*;
use core::ffi::{c_char, c_void};
use std::sync::Mutex;

struct Fake {
    next: u64,
    memories: Vec<(u64, Box<[u8]>)>,
    buffer_sizes: Vec<(u64, u64)>,
    image_sizes: Vec<(u64, u64)>,
    last_alloc_pnext: usize,
}

static FAKE: Mutex<Fake> = Mutex::new(Fake {
    next: 1,
    memories: Vec::new(),
    buffer_sizes: Vec::new(),
    image_sizes: Vec::new(),
    last_alloc_pnext: 0,
});

unsafe extern "system" fn get_props(_d: VkPhysicalDevice, p: *mut VkPhysicalDeviceProperties) {
    *p = core::mem::zeroed();
    (*p).limits.non_coherent_atom_size = 256;
    (*p).limits.buffer_image_granularity = 1;
}

unsafe extern "system" fn get_props2(_d: VkPhysicalDevice, p: *mut VkPhysicalDeviceProperties2) {
    (*p).s_type = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
    get_props(_d, &mut (*p).properties);
}

unsafe extern "system" fn get_mem(_d: VkPhysicalDevice, p: *mut VkPhysicalDeviceMemoryProperties) {
    *p = core::mem::zeroed();
    (*p).memory_heap_count = 1;
    (*p).memory_heaps[0].size = 64 * 1024 * 1024;
    (*p).memory_type_count = 2;
    (*p).memory_types[0].property_flags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
    (*p).memory_types[0].heap_index = 0;
    (*p).memory_types[1].property_flags =
        VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    (*p).memory_types[1].heap_index = 0;
}

unsafe extern "system" fn alloc_mem(
    _dev: VkDevice,
    info: *const VkMemoryAllocateInfo,
    _cb: *const VkAllocationCallbacks,
    out: *mut VkDeviceMemory,
) -> VkResult {
    let mut f = FAKE.lock().unwrap();
    let id = f.next;
    f.next += 1;
    f.last_alloc_pnext = (*info).p_next as usize;
    let n = (*info).allocation_size as usize;
    f.memories
        .push((id, vec![0u8; n.max(1)].into_boxed_slice()));
    *out = id;
    VK_SUCCESS
}

unsafe extern "system" fn free_mem(
    _d: VkDevice,
    _m: VkDeviceMemory,
    _cb: *const VkAllocationCallbacks,
) {
}

unsafe extern "system" fn map(
    _d: VkDevice,
    mem: VkDeviceMemory,
    offset: VkDeviceSize,
    _size: VkDeviceSize,
    _flags: VkFlags,
    pp: *mut *mut c_void,
) -> VkResult {
    let f = FAKE.lock().unwrap();
    let p = f
        .memories
        .iter()
        .find(|(id, _)| *id == mem)
        .map(|(_, v)| v.as_ptr() as *mut u8);
    match p {
        Some(p) => {
            *pp = p.add(offset as usize) as *mut c_void;
            VK_SUCCESS
        }
        None => VK_ERROR_UNKNOWN,
    }
}

unsafe extern "system" fn unmap(_d: VkDevice, _m: VkDeviceMemory) {}

unsafe extern "system" fn flush(_d: VkDevice, _n: u32, _r: *const VkMappedMemoryRange) -> VkResult {
    VK_SUCCESS
}

unsafe extern "system" fn bind_buf(
    _d: VkDevice,
    _b: VkBuffer,
    _m: VkDeviceMemory,
    _o: VkDeviceSize,
) -> VkResult {
    VK_SUCCESS
}

unsafe extern "system" fn bind_img(
    _d: VkDevice,
    _i: VkImage,
    _m: VkDeviceMemory,
    _o: VkDeviceSize,
) -> VkResult {
    VK_SUCCESS
}

unsafe extern "system" fn buf_req(_d: VkDevice, b: VkBuffer, r: *mut VkMemoryRequirements) {
    let f = FAKE.lock().unwrap();
    let size = f
        .buffer_sizes
        .iter()
        .find(|(id, _)| *id == b)
        .map(|(_, s)| *s)
        .unwrap_or(4096)
        .max(256);
    *r = VkMemoryRequirements {
        size,
        alignment: 256,
        memory_type_bits: 0b11,
    };
}

unsafe extern "system" fn img_req(_d: VkDevice, i: VkImage, r: *mut VkMemoryRequirements) {
    let f = FAKE.lock().unwrap();
    let size = f
        .image_sizes
        .iter()
        .find(|(id, _)| *id == i)
        .map(|(_, s)| *s)
        .unwrap_or(8192)
        .max(256);
    *r = VkMemoryRequirements {
        size,
        alignment: 256,
        memory_type_bits: 0b01,
    };
}

unsafe extern "system" fn create_buf(
    _d: VkDevice,
    ci: *const VkBufferCreateInfo,
    _cb: *const VkAllocationCallbacks,
    out: *mut VkBuffer,
) -> VkResult {
    let mut f = FAKE.lock().unwrap();
    *out = f.next;
    f.next += 1;
    let size = if ci.is_null() {
        4096
    } else {
        (*ci).size.max(1)
    };
    f.buffer_sizes.push((*out, size));
    VK_SUCCESS
}

unsafe extern "system" fn destroy_buf(
    _d: VkDevice,
    _b: VkBuffer,
    _cb: *const VkAllocationCallbacks,
) {
}

unsafe extern "system" fn create_img(
    _d: VkDevice,
    ci: *const VkImageCreateInfo,
    _cb: *const VkAllocationCallbacks,
    out: *mut VkImage,
) -> VkResult {
    let mut f = FAKE.lock().unwrap();
    *out = f.next;
    f.next += 1;
    let size = if ci.is_null() {
        8192
    } else {
        let e = (*ci).extent;
        (e.width as u64)
            .saturating_mul(e.height as u64)
            .saturating_mul(e.depth.max(1) as u64)
            .saturating_mul(4)
            .max(256)
    };
    f.image_sizes.push((*out, size));
    VK_SUCCESS
}

unsafe extern "system" fn destroy_img(
    _d: VkDevice,
    _i: VkImage,
    _cb: *const VkAllocationCallbacks,
) {
}

fn fake_fns() -> VmaVulkanFunctions {
    let mut f = VmaVulkanFunctions::default();
    f.vk_get_physical_device_properties = Some(get_props);
    f.vk_get_physical_device_properties2_khr = Some(get_props2);
    f.vk_get_physical_device_memory_properties = Some(get_mem);
    f.vk_allocate_memory = Some(alloc_mem);
    f.vk_free_memory = Some(free_mem);
    f.vk_map_memory = Some(map);
    f.vk_unmap_memory = Some(unmap);
    f.vk_flush_mapped_memory_ranges = Some(flush);
    f.vk_invalidate_mapped_memory_ranges = Some(flush);
    f.vk_bind_buffer_memory = Some(bind_buf);
    f.vk_bind_image_memory = Some(bind_img);
    f.vk_get_buffer_memory_requirements = Some(buf_req);
    f.vk_get_image_memory_requirements = Some(img_req);
    f.vk_create_buffer = Some(create_buf);
    f.vk_destroy_buffer = Some(destroy_buf);
    f.vk_create_image = Some(create_img);
    f.vk_destroy_image = Some(destroy_img);
    f.vk_get_instance_proc_addr = Some(gipa);
    f.vk_get_device_proc_addr = Some(gdpa);
    f
}

unsafe extern "system" fn gipa(_i: VkInstance, _n: *const c_char) -> PFN_vkVoidFunction {
    None
}
unsafe extern "system" fn gdpa(_d: VkDevice, _n: *const c_char) -> PFN_vkVoidFunction {
    None
}

fn allocator_with(flags: u32, vulkan_api_version: u32) -> VmaAllocator {
    let fns = fake_fns();
    let mut ci: VmaAllocatorCreateInfo = unsafe { core::mem::zeroed() };
    ci.physical_device = 1 as _;
    ci.device = 2 as _;
    ci.instance = 3 as _;
    ci.flags = flags;
    ci.vulkan_api_version = vulkan_api_version;
    ci.p_vulkan_functions = &fns;
    let mut a = core::ptr::null_mut();
    unsafe {
        assert_eq!(device::create(&ci, &mut a), VK_SUCCESS);
    }
    a
}

fn fake_allocator() -> VmaAllocator {
    allocator_with(0, 0)
}

/// Blender GHOST + XR: Vulkan 1.2, BDA, memory priority, maintenance4, budget.
fn blender_allocator() -> VmaAllocator {
    allocator_with(
        VMA_ALLOCATOR_CREATE_BUFFER_DEVICE_ADDRESS_BIT
            | VMA_ALLOCATOR_CREATE_EXT_MEMORY_PRIORITY_BIT
            | VMA_ALLOCATOR_CREATE_KHR_MAINTENANCE4_BIT
            | VMA_ALLOCATOR_CREATE_EXT_MEMORY_BUDGET_BIT,
        VK_API_VERSION_1_2,
    )
}

unsafe fn make_buffer(
    a: VmaAllocator,
    size: u64,
    usage: u32,
    aci: &VmaAllocationCreateInfo,
    extra_align: u64,
) -> (VkBuffer, VmaAllocation, VmaAllocationInfo) {
    let mut bci: VkBufferCreateInfo = core::mem::zeroed();
    bci.size = size;
    bci.usage = usage;
    let mut buf = 0u64;
    let mut alloc = core::ptr::null_mut();
    let mut info: VmaAllocationInfo = core::mem::zeroed();
    assert_eq!(
        device::create_buffer(
            a,
            &bci,
            aci,
            extra_align,
            core::ptr::null_mut(),
            &mut buf,
            &mut alloc,
            &mut info
        ),
        VK_SUCCESS
    );
    (buf, alloc, info)
}

unsafe fn make_image(
    a: VmaAllocator,
    w: u32,
    h: u32,
    aci: &VmaAllocationCreateInfo,
) -> (VkImage, VmaAllocation) {
    let mut ici: VkImageCreateInfo = core::mem::zeroed();
    ici.image_type = 1;
    ici.extent = VkExtent3D {
        width: w,
        height: h,
        depth: 1,
    };
    ici.mip_levels = 1;
    ici.array_layers = 1;
    ici.samples = 1;
    ici.tiling = VK_IMAGE_TILING_OPTIMAL;
    ici.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    let mut img = 0u64;
    let mut alloc = core::ptr::null_mut();
    assert_eq!(
        device::create_image(
            a,
            &ici,
            aci,
            core::ptr::null_mut(),
            &mut img,
            &mut alloc,
            core::ptr::null_mut()
        ),
        VK_SUCCESS
    );
    (img, alloc)
}

#[test]
fn vma_version_is_3_4() {
    assert_eq!(VMA_VERSION, vma_make_version(3, 4, 0));
}

#[test]
fn create_destroy_allocator() {
    let a = fake_allocator();
    unsafe {
        device::destroy(a);
    }
}

#[test]
fn create_host_buffer() {
    let a = fake_allocator();
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_AUTO;
    aci.flags =
        VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT | VMA_ALLOCATION_CREATE_MAPPED_BIT;
    unsafe {
        let (buf, alloc, info) = make_buffer(a, 1024, 1, &aci, 1);
        assert!(buf != 0);
        assert!(!alloc.is_null());
        assert!(!info.p_mapped_data.is_null());
        device::destroy_buffer(a, buf, alloc);
        device::destroy(a);
    }
}

#[test]
fn suballoc_two_buffers_same_block() {
    let a = fake_allocator();
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    unsafe {
        let (b1, a1, _) = make_buffer(a, 1024, 1, &aci, 1);
        let (b2, a2, _) = make_buffer(a, 1024, 1, &aci, 1);
        assert_eq!((*a1).memory, (*a2).memory);
        assert_ne!((*a1).offset, (*a2).offset);
        device::destroy_buffer(a, b1, a1);
        device::destroy_buffer(a, b2, a2);
        device::destroy(a);
    }
}

#[test]
fn min_alignment_from_create_info() {
    let a = fake_allocator();
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    aci.min_alignment = 4096;
    unsafe {
        let (b1, a1, _) = make_buffer(a, 64, 1, &aci, 1);
        let (b2, a2, _) = make_buffer(a, 64, 1, &aci, 1);
        assert_eq!((*a1).offset % 4096, 0);
        assert_eq!((*a2).offset % 4096, 0);
        assert_ne!((*a1).offset, (*a2).offset);
        device::destroy_buffer(a, b1, a1);
        device::destroy_buffer(a, b2, a2);
        device::destroy(a);
    }
}

#[test]
fn create_buffer_with_alignment_folds_into_min_alignment() {
    let a = fake_allocator();
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    aci.min_alignment = 512;
    unsafe {
        let (buf, alloc, _) = make_buffer(a, 64, 1, &aci, 2048);
        assert_eq!((*alloc).offset % 2048, 0);
        device::destroy_buffer(a, buf, alloc);
        device::destroy(a);
    }
}

#[test]
fn dedicated_buffer_own_vk_device_memory() {
    let a = fake_allocator();
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    unsafe {
        let mut bci: VkBufferCreateInfo = core::mem::zeroed();
        bci.size = 1024;
        let mut buf = 0u64;
        let mut alloc = core::ptr::null_mut();
        assert_eq!(
            device::create_dedicated_buffer(
                a,
                &bci,
                &aci,
                core::ptr::null_mut(),
                &mut buf,
                &mut alloc,
                core::ptr::null_mut()
            ),
            VK_SUCCESS
        );
        assert!((*alloc).dedicated);
        let mut buf2 = 0u64;
        let mut alloc2 = core::ptr::null_mut();
        assert_eq!(
            device::create_dedicated_buffer(
                a,
                &bci,
                &aci,
                core::ptr::null_mut(),
                &mut buf2,
                &mut alloc2,
                core::ptr::null_mut()
            ),
            VK_SUCCESS
        );
        assert_ne!((*alloc).memory, (*alloc2).memory);
        device::destroy_buffer(a, buf, alloc);
        device::destroy_buffer(a, buf2, alloc2);
        device::destroy(a);
    }
}

#[test]
fn dedicated_pnext_is_chained() {
    let a = fake_allocator();
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    let sentinel = 0xDEC0_DEEEu64;
    unsafe {
        let req = VkMemoryRequirements {
            size: 1024,
            alignment: 256,
            memory_type_bits: 0b11,
        };
        let mut alloc = core::ptr::null_mut();
        assert_eq!(
            device::allocate_dedicated_memory(
                a,
                &req,
                &aci,
                &sentinel as *const u64 as *mut c_void,
                &mut alloc,
                core::ptr::null_mut()
            ),
            VK_SUCCESS
        );
        let pnext = FAKE.lock().unwrap().last_alloc_pnext;
        assert_ne!(pnext, 0);
        device::free_memory(a, alloc);
        device::destroy(a);
    }
}

/// Blender GHOST: many vertex/index/uniform buffers + GPU-only images, then
/// free/realloc like a mesh edit, plus stats, budgets, and defrag ABI.
#[test]
fn blender_ghost_mesh_textures_and_stats() {
    let a = blender_allocator();
    let mut gpu = VmaAllocationCreateInfo::default();
    gpu.usage = VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE;
    let mut mapped = VmaAllocationCreateInfo::default();
    mapped.usage = VMA_MEMORY_USAGE_AUTO;
    mapped.flags =
        VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT | VMA_ALLOCATION_CREATE_MAPPED_BIT;
    let mut img_ci = VmaAllocationCreateInfo::default();
    img_ci.usage = VMA_MEMORY_USAGE_GPU_ONLY;

    unsafe {
        let mut verts = Vec::new();
        for i in 0..48 {
            verts.push(make_buffer(
                a,
                1024 + i * 128,
                VK_BUFFER_USAGE_VERTEX_BUFFER_BIT | VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT,
                &gpu,
                1,
            ));
        }
        let mut indices = Vec::new();
        for i in 0..48 {
            indices.push(make_buffer(
                a,
                512 + i * 64,
                VK_BUFFER_USAGE_INDEX_BUFFER_BIT,
                &gpu,
                1,
            ));
        }
        let mut uniforms = Vec::new();
        for _ in 0..16 {
            let (buf, alloc, info) =
                make_buffer(a, 256, VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT, &mapped, 1);
            assert!(!info.p_mapped_data.is_null());
            core::ptr::write_bytes(info.p_mapped_data, 0xAB, 16);
            uniforms.push((buf, alloc));
        }
        let mut images = Vec::new();
        for i in 0..8 {
            let dim = 32 + i * 16;
            images.push(make_image(a, dim, dim, &img_ci));
        }

        assert!(verts
            .windows(2)
            .any(|w| (*w[0].1).memory == (*w[1].1).memory));
        let mut kept = Vec::new();
        for (i, (buf, alloc, info)) in verts.into_iter().enumerate() {
            if i % 2 == 0 {
                device::destroy_buffer(a, buf, alloc);
            } else {
                kept.push((buf, alloc, info));
            }
        }
        let mut verts = kept;
        for i in 0..12 {
            verts.push(make_buffer(
                a,
                2048 + i * 32,
                VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                &gpu,
                1,
            ));
        }

        let mut stats: VmaTotalStatistics = core::mem::zeroed();
        device::calculate_statistics(a, &mut stats);
        assert!(stats.total.statistics.allocation_count >= 48);
        assert!(stats.total.statistics.allocation_bytes > 0);

        let mut budgets = [VmaBudget::default(); VK_MAX_MEMORY_HEAPS];
        device::get_heap_budgets(a, budgets.as_mut_ptr());
        assert!(budgets[0].statistics.allocation_count > 0);
        assert!(budgets[0].budget > 0);

        let mut ctx = core::ptr::null_mut();
        let di = VmaDefragmentationInfo {
            flags: 0,
            pool: core::ptr::null_mut(),
            max_bytes_per_pass: 0,
            max_allocations_per_pass: 0,
            pfn_break_callback: None,
            p_break_callback_user_data: core::ptr::null_mut(),
        };
        assert_eq!(device::begin_defrag(a, &di, &mut ctx), VK_SUCCESS);
        let mut dstats = VmaDefragmentationStats::default();
        device::end_defrag(a, ctx, &mut dstats);

        for (buf, alloc, _) in verts {
            device::destroy_buffer(a, buf, alloc);
        }
        for (buf, alloc, _) in indices {
            device::destroy_buffer(a, buf, alloc);
        }
        for (buf, alloc) in uniforms {
            device::destroy_buffer(a, buf, alloc);
        }
        for (img, alloc) in images {
            device::destroy_image(a, img, alloc);
        }
        device::destroy(a);
    }
}

/// Blender OpenXR staging: mapped sequential-write buffers, map/unmap, destroy.
#[test]
fn blender_xr_mapped_staging() {
    let a = blender_allocator();
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_AUTO;
    aci.flags =
        VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT | VMA_ALLOCATION_CREATE_MAPPED_BIT;
    unsafe {
        let mut staging = Vec::new();
        for size in [4096u64, 16384, 65536, 256 * 1024] {
            let (buf, alloc, info) =
                make_buffer(a, size, VK_BUFFER_USAGE_TRANSFER_SRC_BIT, &aci, 1);
            assert!(!info.p_mapped_data.is_null());
            core::ptr::write_bytes(info.p_mapped_data, 1, 32);
            let mut p: *mut c_void = core::ptr::null_mut();
            assert_eq!(device::map_memory(a, alloc, &mut p), VK_SUCCESS);
            assert!(!p.is_null());
            device::unmap_memory(a, alloc);
            staging.push((buf, alloc));
        }
        for (buf, alloc) in staging {
            device::destroy_buffer(a, buf, alloc);
        }
        device::destroy(a);
    }
}

#[test]
fn custom_pool_linear_staging() {
    let a = blender_allocator();
    unsafe {
        let mut bits = 0u32;
        let mut find = VmaAllocationCreateInfo::default();
        find.required_flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT;
        assert_eq!(
            device::find_memory_type_index(a, 0b11, &find, &mut bits),
            VK_SUCCESS
        );
        let pci = VmaPoolCreateInfo {
            memory_type_index: bits,
            flags: VMA_POOL_CREATE_LINEAR_ALGORITHM_BIT,
            block_size: 1024 * 1024,
            min_block_count: 1,
            max_block_count: 4,
            priority: 0.0,
            min_allocation_alignment: 256,
            p_memory_allocate_next: core::ptr::null_mut(),
        };
        let mut pool = core::ptr::null_mut();
        assert_eq!(device::create_pool(a, &pci, &mut pool), VK_SUCCESS);
        let mut aci = VmaAllocationCreateInfo::default();
        aci.pool = pool;
        aci.flags = VMA_ALLOCATION_CREATE_MAPPED_BIT
            | VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT;
        let mut bufs = Vec::new();
        for _ in 0..8 {
            bufs.push(make_buffer(
                a,
                4096,
                VK_BUFFER_USAGE_TRANSFER_SRC_BIT,
                &aci,
                1,
            ));
        }
        let mut pst = VmaStatistics::default();
        device::get_pool_statistics(a, pool, &mut pst);
        assert_eq!(pst.allocation_count, 8);
        for (buf, alloc, _) in bufs {
            device::destroy_buffer(a, buf, alloc);
        }
        device::destroy_pool(a, pool);
        device::destroy(a);
    }
}

#[test]
fn dedicated_image() {
    let a = blender_allocator();
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    unsafe {
        let mut ici: VkImageCreateInfo = core::mem::zeroed();
        ici.extent = VkExtent3D {
            width: 128,
            height: 128,
            depth: 1,
        };
        ici.mip_levels = 1;
        ici.array_layers = 1;
        ici.samples = 1;
        ici.usage = VK_IMAGE_USAGE_SAMPLED_BIT;
        let mut img = 0u64;
        let mut alloc = core::ptr::null_mut();
        assert_eq!(
            device::create_dedicated_image(
                a,
                &ici,
                &aci,
                core::ptr::null_mut(),
                &mut img,
                &mut alloc,
                core::ptr::null_mut()
            ),
            VK_SUCCESS
        );
        assert!((*alloc).dedicated);
        device::destroy_image(a, img, alloc);
        device::destroy(a);
    }
}

#[test]
fn hundreds_of_buffer_alloc_free() {
    let a = blender_allocator();
    let mut gpu = VmaAllocationCreateInfo::default();
    gpu.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    unsafe {
        let mut bufs = Vec::new();
        for i in 0..256 {
            bufs.push(make_buffer(
                a,
                128 + (i as u64) * 16,
                VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                &gpu,
                1,
            ));
        }
        for (i, (buf, alloc, _)) in bufs.iter().enumerate() {
            if i % 2 == 0 {
                device::destroy_buffer(a, *buf, *alloc);
            }
        }
        for i in 0..64 {
            let (buf, alloc, _) = make_buffer(a, 512, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT, &gpu, 1);
            device::destroy_buffer(a, buf, alloc);
            let _ = i;
        }
        for (i, (buf, alloc, _)) in bufs.iter().enumerate() {
            if i % 2 == 1 {
                device::destroy_buffer(a, *buf, *alloc);
            }
        }
        let mut stats: VmaTotalStatistics = core::mem::zeroed();
        device::calculate_statistics(a, &mut stats);
        device::destroy(a);
    }
}

#[test]
fn general_pool_vs_linear() {
    let a = blender_allocator();
    unsafe {
        let mut bits = 0u32;
        let mut find = VmaAllocationCreateInfo::default();
        find.required_flags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
        assert_eq!(
            device::find_memory_type_index(a, 0b11, &find, &mut bits),
            VK_SUCCESS
        );
        let pci = VmaPoolCreateInfo {
            memory_type_index: bits,
            flags: 0,
            block_size: 1024 * 1024,
            min_block_count: 1,
            max_block_count: 4,
            priority: 0.0,
            min_allocation_alignment: 256,
            p_memory_allocate_next: core::ptr::null_mut(),
        };
        let mut pool = core::ptr::null_mut();
        assert_eq!(device::create_pool(a, &pci, &mut pool), VK_SUCCESS);
        let mut aci = VmaAllocationCreateInfo::default();
        aci.pool = pool;
        let mut bufs = Vec::new();
        for _ in 0..32 {
            bufs.push(make_buffer(
                a,
                1024,
                VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                &aci,
                1,
            ));
        }
        let mut pst = VmaStatistics::default();
        device::get_pool_statistics(a, pool, &mut pst);
        assert_eq!(pst.allocation_count, 32);
        for (buf, alloc, _) in bufs {
            device::destroy_buffer(a, buf, alloc);
        }
        device::destroy_pool(a, pool);
        device::destroy(a);
    }
}

#[test]
fn safe_allocator_allocation_drop() {
    let fns = fake_fns();
    let mut ci: VmaAllocatorCreateInfo = unsafe { core::mem::zeroed() };
    ci.physical_device = 1 as _;
    ci.device = 2 as _;
    ci.instance = 3 as _;
    ci.p_vulkan_functions = &fns;
    let a = unsafe { crate::safe::Allocator::new(&ci) }.expect("allocator");
    let req = VkMemoryRequirements {
        size: 256,
        alignment: 16,
        memory_type_bits: 0b11,
    };
    let mut create = VmaAllocationCreateInfo::default();
    create.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    {
        let x = a.allocate(&req, &create).expect("x");
        let y = a.allocate(&req, &create).expect("y");
        assert!(x.size() >= 256);
        assert!(y.size() >= 256);
        assert_ne!((x.offset(), x.as_raw()), (y.offset(), y.as_raw()));
    }
}
