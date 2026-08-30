//! `Vma*` types matching AMD `vk_mem_alloc.h` 3.3 (Vulkan 1.1+ function table).

use crate::vk::*;
use core::ffi::{c_char, c_void};

pub type VmaAllocator = *mut crate::device::Allocator;
pub type VmaPool = *mut crate::device::Pool;
pub type VmaAllocation = *mut crate::device::Allocation;
pub type VmaDefragmentationContext = *mut crate::device::Defrag;
pub type VmaVirtualBlock = *mut crate::virtual_block::VirtualBlock;
pub type VmaVirtualAllocation = u64;

pub const VMA_ALLOCATOR_CREATE_EXTERNALLY_SYNCHRONIZED_BIT: u32 = 0x1;
pub const VMA_ALLOCATOR_CREATE_KHR_DEDICATED_ALLOCATION_BIT: u32 = 0x2;
pub const VMA_ALLOCATOR_CREATE_KHR_BIND_MEMORY2_BIT: u32 = 0x4;
pub const VMA_ALLOCATOR_CREATE_EXT_MEMORY_BUDGET_BIT: u32 = 0x8;
pub const VMA_ALLOCATOR_CREATE_AMD_DEVICE_COHERENT_MEMORY_BIT: u32 = 0x10;
pub const VMA_ALLOCATOR_CREATE_BUFFER_DEVICE_ADDRESS_BIT: u32 = 0x20;
pub const VMA_ALLOCATOR_CREATE_EXT_MEMORY_PRIORITY_BIT: u32 = 0x40;
pub const VMA_ALLOCATOR_CREATE_KHR_MAINTENANCE4_BIT: u32 = 0x80;
pub const VMA_ALLOCATOR_CREATE_KHR_MAINTENANCE5_BIT: u32 = 0x100;
pub const VMA_ALLOCATOR_CREATE_KHR_EXTERNAL_MEMORY_WIN32_BIT: u32 = 0x200;

pub const VMA_MEMORY_USAGE_UNKNOWN: u32 = 0;
pub const VMA_MEMORY_USAGE_GPU_ONLY: u32 = 1;
pub const VMA_MEMORY_USAGE_CPU_ONLY: u32 = 2;
pub const VMA_MEMORY_USAGE_CPU_TO_GPU: u32 = 3;
pub const VMA_MEMORY_USAGE_GPU_TO_CPU: u32 = 4;
pub const VMA_MEMORY_USAGE_CPU_COPY: u32 = 5;
pub const VMA_MEMORY_USAGE_GPU_LAZILY_ALLOCATED: u32 = 6;
pub const VMA_MEMORY_USAGE_AUTO: u32 = 7;
pub const VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE: u32 = 8;
pub const VMA_MEMORY_USAGE_AUTO_PREFER_HOST: u32 = 9;

pub const VMA_ALLOCATION_CREATE_DEDICATED_MEMORY_BIT: u32 = 0x1;
pub const VMA_ALLOCATION_CREATE_NEVER_ALLOCATE_BIT: u32 = 0x2;
pub const VMA_ALLOCATION_CREATE_MAPPED_BIT: u32 = 0x4;
pub const VMA_ALLOCATION_CREATE_USER_DATA_COPY_STRING_BIT: u32 = 0x20;
pub const VMA_ALLOCATION_CREATE_UPPER_ADDRESS_BIT: u32 = 0x40;
pub const VMA_ALLOCATION_CREATE_DONT_BIND_BIT: u32 = 0x80;
pub const VMA_ALLOCATION_CREATE_WITHIN_BUDGET_BIT: u32 = 0x100;
pub const VMA_ALLOCATION_CREATE_CAN_ALIAS_BIT: u32 = 0x200;
pub const VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT: u32 = 0x400;
pub const VMA_ALLOCATION_CREATE_HOST_ACCESS_RANDOM_BIT: u32 = 0x800;
pub const VMA_ALLOCATION_CREATE_HOST_ACCESS_ALLOW_TRANSFER_INSTEAD_BIT: u32 = 0x1000;
pub const VMA_ALLOCATION_CREATE_STRATEGY_MIN_MEMORY_BIT: u32 = 0x10000;
pub const VMA_ALLOCATION_CREATE_STRATEGY_MIN_TIME_BIT: u32 = 0x20000;
pub const VMA_ALLOCATION_CREATE_STRATEGY_MIN_OFFSET_BIT: u32 = 0x40000;
pub const VMA_ALLOCATION_CREATE_STRATEGY_MASK: u32 = 0x70000;

pub const VMA_POOL_CREATE_IGNORE_BUFFER_IMAGE_GRANULARITY_BIT: u32 = 0x2;
pub const VMA_POOL_CREATE_LINEAR_ALGORITHM_BIT: u32 = 0x4;

pub const VMA_VIRTUAL_BLOCK_CREATE_LINEAR_ALGORITHM_BIT: u32 = 0x1;

pub const VMA_DEFRAGMENTATION_MOVE_OPERATION_COPY: i32 = 0;
pub const VMA_DEFRAGMENTATION_MOVE_OPERATION_IGNORE: i32 = 1;
pub const VMA_DEFRAGMENTATION_MOVE_OPERATION_DESTROY: i32 = 2;

pub type PFN_vmaAllocateDeviceMemoryFunction = Option<
    unsafe extern "system" fn(VmaAllocator, u32, VkDeviceMemory, VkDeviceSize, *mut c_void),
>;
pub type PFN_vmaFreeDeviceMemoryFunction = Option<
    unsafe extern "system" fn(VmaAllocator, u32, VkDeviceMemory, VkDeviceSize, *mut c_void),
>;
pub type PFN_vmaCheckDefragmentationBreakFunction =
    Option<unsafe extern "system" fn(*mut c_void) -> VkBool32>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaDeviceMemoryCallbacks {
    pub pfn_allocate: PFN_vmaAllocateDeviceMemoryFunction,
    pub pfn_free: PFN_vmaFreeDeviceMemoryFunction,
    pub p_user_data: *mut c_void,
}

/// Layout matches VMA compiled with Vulkan ≥ 1.1 (dedicated, bind2, budget) and 1.3 maintenance4.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaVulkanFunctions {
    pub vk_get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
    pub vk_get_device_proc_addr: PFN_vkGetDeviceProcAddr,
    pub vk_get_physical_device_properties: PFN_vkGetPhysicalDeviceProperties,
    pub vk_get_physical_device_memory_properties: PFN_vkGetPhysicalDeviceMemoryProperties,
    pub vk_allocate_memory: PFN_vkAllocateMemory,
    pub vk_free_memory: PFN_vkFreeMemory,
    pub vk_map_memory: PFN_vkMapMemory,
    pub vk_unmap_memory: PFN_vkUnmapMemory,
    pub vk_flush_mapped_memory_ranges: PFN_vkFlushMappedMemoryRanges,
    pub vk_invalidate_mapped_memory_ranges: PFN_vkInvalidateMappedMemoryRanges,
    pub vk_bind_buffer_memory: PFN_vkBindBufferMemory,
    pub vk_bind_image_memory: PFN_vkBindImageMemory,
    pub vk_get_buffer_memory_requirements: PFN_vkGetBufferMemoryRequirements,
    pub vk_get_image_memory_requirements: PFN_vkGetImageMemoryRequirements,
    pub vk_create_buffer: PFN_vkCreateBuffer,
    pub vk_destroy_buffer: PFN_vkDestroyBuffer,
    pub vk_create_image: PFN_vkCreateImage,
    pub vk_destroy_image: PFN_vkDestroyImage,
    pub vk_cmd_copy_buffer: PFN_vkCmdCopyBuffer,
    pub vk_get_buffer_memory_requirements2_khr: PFN_vkGetBufferMemoryRequirements2,
    pub vk_get_image_memory_requirements2_khr: PFN_vkGetImageMemoryRequirements2,
    pub vk_bind_buffer_memory2_khr: PFN_vkBindBufferMemory2,
    pub vk_bind_image_memory2_khr: PFN_vkBindImageMemory2,
    pub vk_get_physical_device_memory_properties2_khr: PFN_vkGetPhysicalDeviceMemoryProperties2,
    pub vk_get_device_buffer_memory_requirements: PFN_vkGetDeviceBufferMemoryRequirements,
    pub vk_get_device_image_memory_requirements: PFN_vkGetDeviceImageMemoryRequirements,
    pub vk_get_memory_win32_handle_khr: *mut c_void,
}

impl Default for VmaVulkanFunctions {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaAllocatorCreateInfo {
    pub flags: u32,
    pub physical_device: VkPhysicalDevice,
    pub device: VkDevice,
    pub preferred_large_heap_block_size: VkDeviceSize,
    pub p_allocation_callbacks: *const VkAllocationCallbacks,
    pub p_device_memory_callbacks: *const VmaDeviceMemoryCallbacks,
    pub p_heap_size_limit: *const VkDeviceSize,
    pub p_vulkan_functions: *const VmaVulkanFunctions,
    pub instance: VkInstance,
    pub vulkan_api_version: u32,
    pub p_type_external_memory_handle_types: *const u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaAllocatorInfo {
    pub instance: VkInstance,
    pub physical_device: VkPhysicalDevice,
    pub device: VkDevice,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VmaStatistics {
    pub block_count: u32,
    pub allocation_count: u32,
    pub block_bytes: VkDeviceSize,
    pub allocation_bytes: VkDeviceSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaDetailedStatistics {
    pub statistics: VmaStatistics,
    pub unused_range_count: u32,
    pub allocation_size_min: VkDeviceSize,
    pub allocation_size_max: VkDeviceSize,
    pub unused_range_size_min: VkDeviceSize,
    pub unused_range_size_max: VkDeviceSize,
}

impl Default for VmaDetailedStatistics {
    fn default() -> Self {
        Self {
            statistics: VmaStatistics::default(),
            unused_range_count: 0,
            allocation_size_min: VK_WHOLE_SIZE,
            allocation_size_max: 0,
            unused_range_size_min: VK_WHOLE_SIZE,
            unused_range_size_max: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaTotalStatistics {
    pub memory_type: [VmaDetailedStatistics; VK_MAX_MEMORY_TYPES],
    pub memory_heap: [VmaDetailedStatistics; VK_MAX_MEMORY_HEAPS],
    pub total: VmaDetailedStatistics,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VmaBudget {
    pub statistics: VmaStatistics,
    pub usage: VkDeviceSize,
    pub budget: VkDeviceSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaAllocationCreateInfo {
    pub flags: u32,
    pub usage: u32,
    pub required_flags: VkFlags,
    pub preferred_flags: VkFlags,
    pub memory_type_bits: u32,
    pub pool: VmaPool,
    pub p_user_data: *mut c_void,
    pub priority: f32,
}

impl Default for VmaAllocationCreateInfo {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaPoolCreateInfo {
    pub memory_type_index: u32,
    pub flags: u32,
    pub block_size: VkDeviceSize,
    pub min_block_count: usize,
    pub max_block_count: usize,
    pub priority: f32,
    pub min_allocation_alignment: VkDeviceSize,
    pub p_memory_allocate_next: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaAllocationInfo {
    pub memory_type: u32,
    pub device_memory: VkDeviceMemory,
    pub offset: VkDeviceSize,
    pub size: VkDeviceSize,
    pub p_mapped_data: *mut c_void,
    pub p_user_data: *mut c_void,
    pub p_name: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaAllocationInfo2 {
    pub allocation_info: VmaAllocationInfo,
    pub block_size: VkDeviceSize,
    pub dedicated_memory: VkBool32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaDefragmentationInfo {
    pub flags: u32,
    pub pool: VmaPool,
    pub max_bytes_per_pass: VkDeviceSize,
    pub max_allocations_per_pass: u32,
    pub pfn_break_callback: PFN_vmaCheckDefragmentationBreakFunction,
    pub p_break_callback_user_data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaDefragmentationMove {
    pub operation: i32,
    pub src_allocation: VmaAllocation,
    pub dst_tmp_allocation: VmaAllocation,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaDefragmentationPassMoveInfo {
    pub move_count: u32,
    pub p_moves: *mut VmaDefragmentationMove,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VmaDefragmentationStats {
    pub bytes_moved: VkDeviceSize,
    pub bytes_freed: VkDeviceSize,
    pub allocations_moved: u32,
    pub device_memory_blocks_freed: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaVirtualBlockCreateInfo {
    pub size: VkDeviceSize,
    pub flags: u32,
    pub p_allocation_callbacks: *const VkAllocationCallbacks,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaVirtualAllocationCreateInfo {
    pub size: VkDeviceSize,
    pub alignment: VkDeviceSize,
    pub flags: u32,
    pub p_user_data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmaVirtualAllocationInfo {
    pub offset: VkDeviceSize,
    pub size: VkDeviceSize,
    pub p_user_data: *mut c_void,
}
