//! Pure-Rust Vulkan Memory Allocator with AMD VMA **3.4** C ABI.
//!
//! Same `vma*` / `Vma*` types as AMD [`vk_mem_alloc.h`](https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator)
//! v3.4.0. GPU heaps are suballocated from `VkDeviceMemory` blocks using the
//! same first/best-fit (and linear) algorithm as the virtual allocator.
//!
//! Vulkan is called only through [`VmaVulkanFunctions`] (or `libvulkan.so.1`
//! via `vkGetInstanceProcAddr`). The virtual allocator ([`virtual_block`])
//! needs no GPU.
//!
//! # Layout
//!
//! | Module | Role |
//! |--------|------|
//! | [`types`] | `Vma*` structs/flags matching the C header |
//! | [`device`] | Default pools, custom pools, dedicated blocks, buffer/image helpers |
//! | [`safe`] | Owned [`safe::Allocator`] / [`safe::Allocation`] (C ABI stays in `vma-c`) |
//! | [`virtual_block`] | Offset allocator with no `VkDeviceMemory` |
//! | [`free_list`] | Coalescing free ranges (first-fit / best-fit / min-offset) |
//! | [`load`] | Fill [`VmaVulkanFunctions`] from the create-info table or `libvulkan` |
//! | [`vk`] | Vulkan 64-bit types used by the ABI |
//!
//! # Version
//!
//! [`VMA_VERSION`] is `VK_MAKE_VERSION(3, 4, 0)`. SONAME stays
//! `libVulkanMemoryAllocator.so.3` (major 3).

#![allow(non_camel_case_types)]
#![allow(dangerous_implicit_autorefs)]

pub mod device;
pub mod free_list;
pub mod load;
pub mod safe;
pub mod types;
#[cfg(any(kani, test))]
mod verify;
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
