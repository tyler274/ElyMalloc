//! In-process fake Vulkan for allocator tests (no GPU).

use crate::device;
use crate::types::*;
use crate::vk::*;
use core::ffi::{c_char, c_void};
use std::sync::Mutex;

struct Fake {
    next: u64,
    memories: Vec<(u64, Box<[u8]>)>,
}

static FAKE: Mutex<Fake> = Mutex::new(Fake {
    next: 1,
    memories: Vec::new(),
});

unsafe extern "system" fn get_props(_d: VkPhysicalDevice, p: *mut VkPhysicalDeviceProperties) {
    *p = core::mem::zeroed();
    (*p).limits.non_coherent_atom_size = 256;
    (*p).limits.buffer_image_granularity = 1;
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
    let n = (*info).allocation_size as usize;
    f.memories.push((id, vec![0u8; n.max(1)].into_boxed_slice()));
    *out = id;
    VK_SUCCESS
}

unsafe extern "system" fn free_mem(_d: VkDevice, _m: VkDeviceMemory, _cb: *const VkAllocationCallbacks) {}

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

unsafe extern "system" fn buf_req(_d: VkDevice, _b: VkBuffer, r: *mut VkMemoryRequirements) {
    *r = VkMemoryRequirements {
        size: 4096,
        alignment: 256,
        memory_type_bits: 0b11,
    };
}

unsafe extern "system" fn img_req(_d: VkDevice, _i: VkImage, r: *mut VkMemoryRequirements) {
    *r = VkMemoryRequirements {
        size: 8192,
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
    let _ = ci;
    VK_SUCCESS
}

unsafe extern "system" fn destroy_buf(_d: VkDevice, _b: VkBuffer, _cb: *const VkAllocationCallbacks) {}

unsafe extern "system" fn create_img(
    _d: VkDevice,
    _ci: *const VkImageCreateInfo,
    _cb: *const VkAllocationCallbacks,
    out: *mut VkImage,
) -> VkResult {
    let mut f = FAKE.lock().unwrap();
    *out = f.next;
    f.next += 1;
    VK_SUCCESS
}

unsafe extern "system" fn destroy_img(_d: VkDevice, _i: VkImage, _cb: *const VkAllocationCallbacks) {}

fn fake_fns() -> VmaVulkanFunctions {
    let mut f = VmaVulkanFunctions::default();
    f.vk_get_physical_device_properties = Some(get_props);
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

fn fake_allocator() -> VmaAllocator {
    let fns = fake_fns();
    let mut ci: VmaAllocatorCreateInfo = unsafe { core::mem::zeroed() };
    ci.physical_device = 1 as _;
    ci.device = 2 as _;
    ci.instance = 3 as _;
    ci.p_vulkan_functions = &fns;
    let mut a = core::ptr::null_mut();
    unsafe {
        assert_eq!(device::create(&ci, &mut a), VK_SUCCESS);
    }
    a
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
    let mut bci: VkBufferCreateInfo = unsafe { core::mem::zeroed() };
    bci.size = 1024;
    bci.usage = 1;
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_AUTO;
    aci.flags = VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT
        | VMA_ALLOCATION_CREATE_MAPPED_BIT;
    let mut buf = 0u64;
    let mut alloc = core::ptr::null_mut();
    let mut info: VmaAllocationInfo = unsafe { core::mem::zeroed() };
    unsafe {
        assert_eq!(
            device::create_buffer(a, &bci, &aci, 1, &mut buf, &mut alloc, &mut info),
            VK_SUCCESS
        );
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
    let mut bci: VkBufferCreateInfo = unsafe { core::mem::zeroed() };
    bci.size = 1024;
    let mut aci = VmaAllocationCreateInfo::default();
    aci.usage = VMA_MEMORY_USAGE_GPU_ONLY;
    let mut b1 = 0;
    let mut b2 = 0;
    let mut a1 = core::ptr::null_mut();
    let mut a2 = core::ptr::null_mut();
    unsafe {
        assert_eq!(
            device::create_buffer(a, &bci, &aci, 1, &mut b1, &mut a1, core::ptr::null_mut()),
            VK_SUCCESS
        );
        assert_eq!(
            device::create_buffer(a, &bci, &aci, 1, &mut b2, &mut a2, core::ptr::null_mut()),
            VK_SUCCESS
        );
        assert_eq!((*a1).memory, (*a2).memory);
        assert_ne!((*a1).offset, (*a2).offset);
        device::destroy_buffer(a, b1, a1);
        device::destroy_buffer(a, b2, a2);
        device::destroy(a);
    }
}
