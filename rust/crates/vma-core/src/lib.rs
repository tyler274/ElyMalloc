//! Pure-Rust Vulkan Memory Allocator with AMD VMA 3.3 C ABI.
//!
//! Same `vma*` / `Vma*` types as `vk_mem_alloc.h`. GPU heaps are suballocated
//! from `VkDeviceMemory` blocks using the same first/best-fit algorithm as
//! the virtual allocator. Vulkan is called only through [`VmaVulkanFunctions`]
//! (or `libvulkan.so.1` via `vkGetInstanceProcAddr`).

#![allow(non_camel_case_types)]
#![allow(dangerous_implicit_autorefs)]

pub mod device;
pub mod free_list;
pub mod load;
pub mod types;
pub mod virtual_block;
pub mod vk;

pub use types::*;
pub use vk::{
    VkBufferCreateInfo, VkDeviceSize, VkImageCreateInfo, VkMemoryRequirements, VkResult,
    VK_ERROR_FEATURE_NOT_PRESENT, VK_ERROR_OUT_OF_DEVICE_MEMORY, VK_ERROR_OUT_OF_HOST_MEMORY,
    VK_INCOMPLETE, VK_SUCCESS, VK_WHOLE_SIZE,
};

#[cfg(test)]
mod mock;
