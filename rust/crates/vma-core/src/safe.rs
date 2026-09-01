//! Safe wrappers around [`crate::device`] handles.
//!
//! [`Allocator`] owns a `VmaAllocator`. [`Allocation`] borrows it and frees
//! on drop. The C ABI in `vma-c` stays a thin `unsafe extern "C"` shim.

use crate::device;
use crate::types::*;
use crate::vk::*;

/// Owned VMA allocator. Destroyed on drop.
pub struct Allocator {
    raw: VmaAllocator,
}

/// Suballocation that cannot outlive its [`Allocator`].
pub struct Allocation<'a> {
    parent: &'a Allocator,
    raw: VmaAllocation,
    offset: u64,
    size: u64,
}

impl Allocator {
    /// Create from AMD VMA create-info (Vulkan handles in `info` must be valid).
    ///
    /// # Safety
    /// Same as [`device::create`]: instance/device/physical device and the
    /// function table remain live for the allocator's lifetime.
    pub unsafe fn new(info: &VmaAllocatorCreateInfo) -> Result<Self, VkResult> {
        let mut raw = core::ptr::null_mut();
        let r = device::create(info, &mut raw);
        if r != VK_SUCCESS {
            return Err(r);
        }
        Ok(Self { raw })
    }

    pub fn as_raw(&self) -> VmaAllocator {
        self.raw
    }

    /// Suballocate `req.size` bytes at `req.alignment` (and create-info minAlignment).
    pub fn allocate(
        &self,
        req: &VkMemoryRequirements,
        create: &VmaAllocationCreateInfo,
    ) -> Result<Allocation<'_>, VkResult> {
        let mut out = core::ptr::null_mut();
        let r = unsafe {
            device::allocate_memory(self.raw, req, create, &mut out, core::ptr::null_mut())
        };
        if r != VK_SUCCESS {
            return Err(r);
        }
        let (offset, size) = unsafe { ((*out).offset, (*out).size) };
        Ok(Allocation {
            parent: self,
            raw: out,
            offset,
            size,
        })
    }
}

impl Drop for Allocator {
    fn drop(&mut self) {
        unsafe {
            device::destroy(self.raw);
        }
        self.raw = core::ptr::null_mut();
    }
}

impl Allocation<'_> {
    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn as_raw(&self) -> VmaAllocation {
        self.raw
    }
}

impl Drop for Allocation<'_> {
    fn drop(&mut self) {
        unsafe {
            device::free_memory(self.parent.raw, self.raw);
        }
        self.raw = core::ptr::null_mut();
    }
}
