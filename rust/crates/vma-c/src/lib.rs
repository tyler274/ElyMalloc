//! C ABI for AMD Vulkan Memory Allocator **3.4** (`vma*` / `Vma*`).
//!
//! Link this `cdylib` / `staticlib` instead of compiling `vk_mem_alloc.h` with
//! `VMA_IMPLEMENTATION`. Include AMD's header (or `include/vk_mem_alloc.h` here)
//! without defining `VMA_IMPLEMENTATION`. SONAME is `libVulkanMemoryAllocator.so.3`.
//!
//! # Safety
//!
//! Every `vma*` export is `unsafe` C ABI. Pointers must match AMD VMA:
//! non-null where the header uses `VMA_NOT_NULL`, allocator/pool/allocation
//! handles from the corresponding create call, and Vulkan objects that remain
//! valid for the duration of the call. Null allocator/allocation is a no-op
//! on destroy/free, as in upstream.

#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

use core::ffi::{c_char, c_void};
use vma_core::device;
use vma_core::virtual_block as vblock;
use vma_core::vk::*;
use vma_core::*;

#[no_mangle]
pub unsafe extern "C" fn vmaImportVulkanFunctionsFromVolk(
    p_allocator_create_info: *const VmaAllocatorCreateInfo,
    p_dst: *mut VmaVulkanFunctions,
) -> VkResult {
    device::import_vulkan_functions_from_volk(p_allocator_create_info, p_dst)
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreateAllocator(
    p_create_info: *const VmaAllocatorCreateInfo,
    p_allocator: *mut VmaAllocator,
) -> VkResult {
    device::create(p_create_info, p_allocator)
}

#[no_mangle]
pub unsafe extern "C" fn vmaDestroyAllocator(allocator: VmaAllocator) {
    device::destroy(allocator);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetAllocatorInfo(
    allocator: VmaAllocator,
    p_allocator_info: *mut VmaAllocatorInfo,
) {
    device::get_allocator_info(allocator, p_allocator_info);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetPhysicalDeviceProperties(
    allocator: VmaAllocator,
    pp_physical_device_properties: *mut *const VkPhysicalDeviceProperties,
) {
    device::get_physical_device_properties(allocator, pp_physical_device_properties);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetMemoryProperties(
    allocator: VmaAllocator,
    pp_physical_device_memory_properties: *mut *const VkPhysicalDeviceMemoryProperties,
) {
    device::get_memory_properties(allocator, pp_physical_device_memory_properties);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetMemoryTypeProperties(
    allocator: VmaAllocator,
    memory_type_index: u32,
    p_flags: *mut VkFlags,
) {
    device::get_memory_type_properties(allocator, memory_type_index, p_flags);
}

#[no_mangle]
pub unsafe extern "C" fn vmaSetCurrentFrameIndex(allocator: VmaAllocator, frame_index: u32) {
    device::set_current_frame_index(allocator, frame_index);
}

#[no_mangle]
pub unsafe extern "C" fn vmaCalculateStatistics(
    allocator: VmaAllocator,
    p_stats: *mut VmaTotalStatistics,
) {
    device::calculate_statistics(allocator, p_stats);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetHeapBudgets(allocator: VmaAllocator, p_budgets: *mut VmaBudget) {
    device::get_heap_budgets(allocator, p_budgets);
}

#[no_mangle]
pub unsafe extern "C" fn vmaFindMemoryTypeIndex(
    allocator: VmaAllocator,
    memory_type_bits: u32,
    p_allocation_create_info: *const VmaAllocationCreateInfo,
    p_memory_type_index: *mut u32,
) -> VkResult {
    device::find_memory_type_index(
        allocator,
        memory_type_bits,
        p_allocation_create_info,
        p_memory_type_index,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaFindMemoryTypeIndexForBufferInfo(
    allocator: VmaAllocator,
    p_buffer_create_info: *const VkBufferCreateInfo,
    p_allocation_create_info: *const VmaAllocationCreateInfo,
    p_memory_type_index: *mut u32,
) -> VkResult {
    device::find_memory_type_index_for_buffer_info(
        allocator,
        p_buffer_create_info,
        p_allocation_create_info,
        p_memory_type_index,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaFindMemoryTypeIndexForImageInfo(
    allocator: VmaAllocator,
    p_image_create_info: *const VkImageCreateInfo,
    p_allocation_create_info: *const VmaAllocationCreateInfo,
    p_memory_type_index: *mut u32,
) -> VkResult {
    device::find_memory_type_index_for_image_info(
        allocator,
        p_image_create_info,
        p_allocation_create_info,
        p_memory_type_index,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreatePool(
    allocator: VmaAllocator,
    p_create_info: *const VmaPoolCreateInfo,
    p_pool: *mut VmaPool,
) -> VkResult {
    device::create_pool(allocator, p_create_info, p_pool)
}

#[no_mangle]
pub unsafe extern "C" fn vmaDestroyPool(allocator: VmaAllocator, pool: VmaPool) {
    device::destroy_pool(allocator, pool);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetPoolStatistics(
    allocator: VmaAllocator,
    pool: VmaPool,
    p_pool_stats: *mut VmaStatistics,
) {
    device::get_pool_statistics(allocator, pool, p_pool_stats);
}

#[no_mangle]
pub unsafe extern "C" fn vmaCalculatePoolStatistics(
    allocator: VmaAllocator,
    pool: VmaPool,
    p_pool_stats: *mut VmaDetailedStatistics,
) {
    device::calculate_pool_statistics(allocator, pool, p_pool_stats);
}

#[no_mangle]
pub unsafe extern "C" fn vmaCheckPoolCorruption(
    allocator: VmaAllocator,
    pool: VmaPool,
) -> VkResult {
    device::check_pool_corruption(allocator, pool)
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetPoolName(
    allocator: VmaAllocator,
    pool: VmaPool,
    pp_name: *mut *const c_char,
) {
    device::get_pool_name(allocator, pool, pp_name);
}

#[no_mangle]
pub unsafe extern "C" fn vmaSetPoolName(
    allocator: VmaAllocator,
    pool: VmaPool,
    p_name: *const c_char,
) {
    device::set_pool_name(allocator, pool, p_name);
}

#[no_mangle]
pub unsafe extern "C" fn vmaAllocateMemory(
    allocator: VmaAllocator,
    p_vk_memory_requirements: *const VkMemoryRequirements,
    p_create_info: *const VmaAllocationCreateInfo,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::allocate_memory(
        allocator,
        p_vk_memory_requirements,
        p_create_info,
        p_allocation,
        p_allocation_info,
    )
}

/// 3.4: dedicated allocation with an extra `VkMemoryAllocateInfo::pNext` chain.
#[no_mangle]
pub unsafe extern "C" fn vmaAllocateDedicatedMemory(
    allocator: VmaAllocator,
    p_vk_memory_requirements: *const VkMemoryRequirements,
    p_create_info: *const VmaAllocationCreateInfo,
    p_memory_allocate_next: *mut c_void,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::allocate_dedicated_memory(
        allocator,
        p_vk_memory_requirements,
        p_create_info,
        p_memory_allocate_next,
        p_allocation,
        p_allocation_info,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaAllocateMemoryPages(
    allocator: VmaAllocator,
    p_vk_memory_requirements: *const VkMemoryRequirements,
    p_create_info: *const VmaAllocationCreateInfo,
    allocation_count: usize,
    p_allocations: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::allocate_memory_pages(
        allocator,
        p_vk_memory_requirements,
        p_create_info,
        allocation_count,
        p_allocations,
        p_allocation_info,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaAllocateMemoryForBuffer(
    allocator: VmaAllocator,
    buffer: VkBuffer,
    p_create_info: *const VmaAllocationCreateInfo,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::allocate_memory_for_buffer(
        allocator,
        buffer,
        p_create_info,
        p_allocation,
        p_allocation_info,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaAllocateMemoryForImage(
    allocator: VmaAllocator,
    image: VkImage,
    p_create_info: *const VmaAllocationCreateInfo,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::allocate_memory_for_image(
        allocator,
        image,
        p_create_info,
        p_allocation,
        p_allocation_info,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaFreeMemory(allocator: VmaAllocator, allocation: VmaAllocation) {
    device::free_memory(allocator, allocation);
}

#[no_mangle]
pub unsafe extern "C" fn vmaFreeMemoryPages(
    allocator: VmaAllocator,
    allocation_count: usize,
    p_allocations: *const VmaAllocation,
) {
    device::free_memory_pages(allocator, allocation_count, p_allocations);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetAllocationInfo(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) {
    device::get_allocation_info(allocator, allocation, p_allocation_info);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetAllocationInfo2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo2,
) {
    device::get_allocation_info2(allocator, allocation, p_allocation_info);
}

#[no_mangle]
pub unsafe extern "C" fn vmaSetAllocationUserData(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    p_user_data: *mut c_void,
) {
    device::set_allocation_user_data(allocator, allocation, p_user_data);
}

#[no_mangle]
pub unsafe extern "C" fn vmaSetAllocationName(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    p_name: *const c_char,
) {
    device::set_allocation_name(allocator, allocation, p_name);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetAllocationMemoryProperties(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    p_flags: *mut VkFlags,
) {
    device::get_allocation_memory_properties(allocator, allocation, p_flags);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetMemoryWin32Handle(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    h_target_process: *mut c_void,
    p_handle: *mut *mut c_void,
) -> VkResult {
    device::get_memory_win32_handle(allocator, allocation, h_target_process, p_handle)
}

/// 3.4: Win32 `vkGetMemoryWin32HandleKHR` helper. On Linux this always returns
/// `VK_ERROR_FEATURE_NOT_PRESENT`.
#[no_mangle]
pub unsafe extern "C" fn vmaGetMemoryWin32Handle2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    handle_type: u32,
    h_target_process: *mut c_void,
    p_handle: *mut *mut c_void,
) -> VkResult {
    device::get_memory_win32_handle2(
        allocator,
        allocation,
        handle_type,
        h_target_process,
        p_handle,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaMapMemory(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    pp_data: *mut *mut c_void,
) -> VkResult {
    device::map_memory(allocator, allocation, pp_data)
}

#[no_mangle]
pub unsafe extern "C" fn vmaUnmapMemory(allocator: VmaAllocator, allocation: VmaAllocation) {
    device::unmap_memory(allocator, allocation);
}

#[no_mangle]
pub unsafe extern "C" fn vmaFlushAllocation(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    offset: VkDeviceSize,
    size: VkDeviceSize,
) -> VkResult {
    device::flush_allocation(allocator, allocation, offset, size)
}

#[no_mangle]
pub unsafe extern "C" fn vmaInvalidateAllocation(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    offset: VkDeviceSize,
    size: VkDeviceSize,
) -> VkResult {
    device::invalidate_allocation(allocator, allocation, offset, size)
}

#[no_mangle]
pub unsafe extern "C" fn vmaFlushAllocations(
    allocator: VmaAllocator,
    allocation_count: u32,
    allocations: *const VmaAllocation,
    offsets: *const VkDeviceSize,
    sizes: *const VkDeviceSize,
) -> VkResult {
    device::flush_allocations(allocator, allocation_count, allocations, offsets, sizes)
}

#[no_mangle]
pub unsafe extern "C" fn vmaInvalidateAllocations(
    allocator: VmaAllocator,
    allocation_count: u32,
    allocations: *const VmaAllocation,
    offsets: *const VkDeviceSize,
    sizes: *const VkDeviceSize,
) -> VkResult {
    device::invalidate_allocations(allocator, allocation_count, allocations, offsets, sizes)
}

#[no_mangle]
pub unsafe extern "C" fn vmaCopyMemoryToAllocation(
    allocator: VmaAllocator,
    p_src_host_pointer: *const c_void,
    dst_allocation: VmaAllocation,
    dst_allocation_local_offset: VkDeviceSize,
    size: VkDeviceSize,
) -> VkResult {
    device::copy_to_allocation(
        allocator,
        p_src_host_pointer,
        dst_allocation,
        dst_allocation_local_offset,
        size,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaCopyAllocationToMemory(
    allocator: VmaAllocator,
    src_allocation: VmaAllocation,
    src_allocation_local_offset: VkDeviceSize,
    p_dst_host_pointer: *mut c_void,
    size: VkDeviceSize,
) -> VkResult {
    device::copy_from_allocation(
        allocator,
        src_allocation,
        src_allocation_local_offset,
        p_dst_host_pointer,
        size,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaCheckCorruption(
    allocator: VmaAllocator,
    memory_type_bits: u32,
) -> VkResult {
    device::check_corruption(allocator, memory_type_bits)
}

#[no_mangle]
pub unsafe extern "C" fn vmaBeginDefragmentation(
    allocator: VmaAllocator,
    p_info: *const VmaDefragmentationInfo,
    p_context: *mut VmaDefragmentationContext,
) -> VkResult {
    device::begin_defrag(allocator, p_info, p_context)
}

#[no_mangle]
pub unsafe extern "C" fn vmaEndDefragmentation(
    allocator: VmaAllocator,
    context: VmaDefragmentationContext,
    p_stats: *mut VmaDefragmentationStats,
) {
    device::end_defrag(allocator, context, p_stats);
}

#[no_mangle]
pub unsafe extern "C" fn vmaBeginDefragmentationPass(
    allocator: VmaAllocator,
    context: VmaDefragmentationContext,
    p_pass_info: *mut VmaDefragmentationPassMoveInfo,
) -> VkResult {
    device::begin_defrag_pass(allocator, context, p_pass_info)
}

#[no_mangle]
pub unsafe extern "C" fn vmaEndDefragmentationPass(
    allocator: VmaAllocator,
    context: VmaDefragmentationContext,
    p_pass_info: *mut VmaDefragmentationPassMoveInfo,
) -> VkResult {
    device::end_defrag_pass(allocator, context, p_pass_info)
}

#[no_mangle]
pub unsafe extern "C" fn vmaBindBufferMemory(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    buffer: VkBuffer,
) -> VkResult {
    device::bind_buffer(allocator, allocation, buffer)
}

#[no_mangle]
pub unsafe extern "C" fn vmaBindBufferMemory2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    allocation_local_offset: VkDeviceSize,
    buffer: VkBuffer,
    p_next: *const c_void,
) -> VkResult {
    device::bind_buffer2(
        allocator,
        allocation,
        allocation_local_offset,
        buffer,
        p_next,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaBindImageMemory(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    image: VkImage,
) -> VkResult {
    device::bind_image(allocator, allocation, image)
}

#[no_mangle]
pub unsafe extern "C" fn vmaBindImageMemory2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    allocation_local_offset: VkDeviceSize,
    image: VkImage,
    p_next: *const c_void,
) -> VkResult {
    device::bind_image2(
        allocator,
        allocation,
        allocation_local_offset,
        image,
        p_next,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreateBuffer(
    allocator: VmaAllocator,
    p_buffer_create_info: *const VkBufferCreateInfo,
    p_allocation_create_info: *const VmaAllocationCreateInfo,
    p_buffer: *mut VkBuffer,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::create_buffer(
        allocator,
        p_buffer_create_info,
        p_allocation_create_info,
        1,
        core::ptr::null_mut(),
        p_buffer,
        p_allocation,
        p_allocation_info,
    )
}

/// Obsolete in 3.4: prefer `VmaAllocationCreateInfo::min_alignment`.
/// This still takes the max of `min_alignment` and the create-info field.
#[no_mangle]
pub unsafe extern "C" fn vmaCreateBufferWithAlignment(
    allocator: VmaAllocator,
    p_buffer_create_info: *const VkBufferCreateInfo,
    p_allocation_create_info: *const VmaAllocationCreateInfo,
    min_alignment: VkDeviceSize,
    p_buffer: *mut VkBuffer,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::create_buffer(
        allocator,
        p_buffer_create_info,
        p_allocation_create_info,
        min_alignment,
        core::ptr::null_mut(),
        p_buffer,
        p_allocation,
        p_allocation_info,
    )
}

/// 3.4: dedicated buffer; implies `VMA_ALLOCATION_CREATE_DEDICATED_MEMORY_BIT`.
#[no_mangle]
pub unsafe extern "C" fn vmaCreateDedicatedBuffer(
    allocator: VmaAllocator,
    p_buffer_create_info: *const VkBufferCreateInfo,
    p_allocation_create_info: *const VmaAllocationCreateInfo,
    p_memory_allocate_next: *mut c_void,
    p_buffer: *mut VkBuffer,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::create_dedicated_buffer(
        allocator,
        p_buffer_create_info,
        p_allocation_create_info,
        p_memory_allocate_next,
        p_buffer,
        p_allocation,
        p_allocation_info,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreateAliasingBuffer(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    p_buffer_create_info: *const VkBufferCreateInfo,
    p_buffer: *mut VkBuffer,
) -> VkResult {
    device::aliasing_buffer(allocator, allocation, 0, p_buffer_create_info, p_buffer)
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreateAliasingBuffer2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    allocation_local_offset: VkDeviceSize,
    p_buffer_create_info: *const VkBufferCreateInfo,
    p_buffer: *mut VkBuffer,
) -> VkResult {
    device::aliasing_buffer(
        allocator,
        allocation,
        allocation_local_offset,
        p_buffer_create_info,
        p_buffer,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaDestroyBuffer(
    allocator: VmaAllocator,
    buffer: VkBuffer,
    allocation: VmaAllocation,
) {
    device::destroy_buffer(allocator, buffer, allocation);
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreateImage(
    allocator: VmaAllocator,
    p_image_create_info: *const VkImageCreateInfo,
    p_allocation_create_info: *const VmaAllocationCreateInfo,
    p_image: *mut VkImage,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::create_image(
        allocator,
        p_image_create_info,
        p_allocation_create_info,
        core::ptr::null_mut(),
        p_image,
        p_allocation,
        p_allocation_info,
    )
}

/// 3.4: dedicated image; implies `VMA_ALLOCATION_CREATE_DEDICATED_MEMORY_BIT`.
#[no_mangle]
pub unsafe extern "C" fn vmaCreateDedicatedImage(
    allocator: VmaAllocator,
    p_image_create_info: *const VkImageCreateInfo,
    p_allocation_create_info: *const VmaAllocationCreateInfo,
    p_memory_allocate_next: *mut c_void,
    p_image: *mut VkImage,
    p_allocation: *mut VmaAllocation,
    p_allocation_info: *mut VmaAllocationInfo,
) -> VkResult {
    device::create_dedicated_image(
        allocator,
        p_image_create_info,
        p_allocation_create_info,
        p_memory_allocate_next,
        p_image,
        p_allocation,
        p_allocation_info,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreateAliasingImage(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    p_image_create_info: *const VkImageCreateInfo,
    p_image: *mut VkImage,
) -> VkResult {
    device::aliasing_image(allocator, allocation, 0, p_image_create_info, p_image)
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreateAliasingImage2(
    allocator: VmaAllocator,
    allocation: VmaAllocation,
    allocation_local_offset: VkDeviceSize,
    p_image_create_info: *const VkImageCreateInfo,
    p_image: *mut VkImage,
) -> VkResult {
    device::aliasing_image(
        allocator,
        allocation,
        allocation_local_offset,
        p_image_create_info,
        p_image,
    )
}

#[no_mangle]
pub unsafe extern "C" fn vmaDestroyImage(
    allocator: VmaAllocator,
    image: VkImage,
    allocation: VmaAllocation,
) {
    device::destroy_image(allocator, image, allocation);
}

#[no_mangle]
pub unsafe extern "C" fn vmaCreateVirtualBlock(
    p_create_info: *const VmaVirtualBlockCreateInfo,
    p_virtual_block: *mut VmaVirtualBlock,
) -> VkResult {
    vblock::create(p_create_info, p_virtual_block)
}

#[no_mangle]
pub unsafe extern "C" fn vmaDestroyVirtualBlock(virtual_block: VmaVirtualBlock) {
    vblock::destroy(virtual_block);
}

#[no_mangle]
pub unsafe extern "C" fn vmaIsVirtualBlockEmpty(virtual_block: VmaVirtualBlock) -> VkBool32 {
    vblock::is_empty(virtual_block)
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetVirtualAllocationInfo(
    virtual_block: VmaVirtualBlock,
    allocation: VmaVirtualAllocation,
    p_virtual_alloc_info: *mut VmaVirtualAllocationInfo,
) {
    vblock::get_info(virtual_block, allocation, p_virtual_alloc_info);
}

#[no_mangle]
pub unsafe extern "C" fn vmaVirtualAllocate(
    virtual_block: VmaVirtualBlock,
    p_create_info: *const VmaVirtualAllocationCreateInfo,
    p_allocation: *mut VmaVirtualAllocation,
    p_offset: *mut VkDeviceSize,
) -> VkResult {
    vblock::allocate(virtual_block, p_create_info, p_allocation, p_offset)
}

#[no_mangle]
pub unsafe extern "C" fn vmaVirtualFree(
    virtual_block: VmaVirtualBlock,
    allocation: VmaVirtualAllocation,
) {
    vblock::free(virtual_block, allocation);
}

#[no_mangle]
pub unsafe extern "C" fn vmaClearVirtualBlock(virtual_block: VmaVirtualBlock) {
    if !virtual_block.is_null() {
        (*virtual_block).clear();
    }
}

#[no_mangle]
pub unsafe extern "C" fn vmaSetVirtualAllocationUserData(
    virtual_block: VmaVirtualBlock,
    allocation: VmaVirtualAllocation,
    p_user_data: *mut c_void,
) {
    vblock::set_user_data(virtual_block, allocation, p_user_data);
}

#[no_mangle]
pub unsafe extern "C" fn vmaGetVirtualBlockStatistics(
    virtual_block: VmaVirtualBlock,
    p_stats: *mut VmaStatistics,
) {
    if virtual_block.is_null() || p_stats.is_null() {
        return;
    }
    *p_stats = (*virtual_block).stats();
}

#[no_mangle]
pub unsafe extern "C" fn vmaCalculateVirtualBlockStatistics(
    virtual_block: VmaVirtualBlock,
    p_stats: *mut VmaDetailedStatistics,
) {
    if virtual_block.is_null() || p_stats.is_null() {
        return;
    }
    *p_stats = (*virtual_block).detailed();
}

#[no_mangle]
pub unsafe extern "C" fn vmaBuildVirtualBlockStatsString(
    virtual_block: VmaVirtualBlock,
    pp_stats_string: *mut *mut c_char,
    _detailed_map: VkBool32,
) {
    if pp_stats_string.is_null() {
        return;
    }
    *pp_stats_string = vblock::stats_string(virtual_block);
}

#[no_mangle]
pub unsafe extern "C" fn vmaFreeVirtualBlockStatsString(
    _virtual_block: VmaVirtualBlock,
    p_stats_string: *mut c_char,
) {
    vblock::free_string(p_stats_string);
}

#[no_mangle]
pub unsafe extern "C" fn vmaBuildStatsString(
    allocator: VmaAllocator,
    pp_stats_string: *mut *mut c_char,
    detailed_map: VkBool32,
) {
    device::build_stats_string(allocator, pp_stats_string, detailed_map);
}

#[no_mangle]
pub unsafe extern "C" fn vmaFreeStatsString(allocator: VmaAllocator, p_stats_string: *mut c_char) {
    device::free_stats_string(allocator, p_stats_string);
}
