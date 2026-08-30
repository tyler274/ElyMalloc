//! Virtual allocator (C `group_virtual`): same algorithm, no GPU memory.

use crate::free_list::{align_up, FreeList};
use crate::types::*;
use crate::vk::{VK_ERROR_OUT_OF_DEVICE_MEMORY, VK_FALSE, VK_SUCCESS, VK_TRUE};
use std::collections::HashMap;
use std::ffi::{c_char, c_void};

pub struct VirtualAlloc {
    pub offset: u64,
    pub size: u64,
    pub user_data: *mut c_void,
}

pub struct VirtualBlock {
    pub size: u64,
    pub flags: u32,
    pub free: FreeList,
    pub allocs: HashMap<u64, VirtualAlloc>,
    pub next_id: u64,
}

impl VirtualBlock {
    pub fn new(size: u64, flags: u32) -> Self {
        Self {
            size,
            flags,
            free: FreeList::new(size, flags & VMA_VIRTUAL_BLOCK_CREATE_LINEAR_ALGORITHM_BIT != 0),
            allocs: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn allocate(
        &mut self,
        size: u64,
        alignment: u64,
        flags: u32,
        user: *mut c_void,
    ) -> Option<(u64, u64)> {
        let alignment = if alignment == 0 { 1 } else { alignment };
        if !alignment.is_power_of_two() && alignment != 1 {
            return None;
        }
        let off = self.free.alloc(size, alignment, flags)?;
        let id = self.next_id;
        self.next_id += 1;
        self.allocs.insert(
            id,
            VirtualAlloc {
                offset: off,
                size,
                user_data: user,
            },
        );
        Some((id, off))
    }

    pub fn free(&mut self, id: u64) {
        if let Some(a) = self.allocs.remove(&id) {
            self.free.free(a.offset, a.size);
        }
    }

    pub fn clear(&mut self) {
        self.allocs.clear();
        self.free.clear();
    }

    pub fn stats(&self) -> VmaStatistics {
        VmaStatistics {
            block_count: 1,
            allocation_count: self.allocs.len() as u32,
            block_bytes: self.size,
            allocation_bytes: self.allocs.values().map(|a| a.size).sum(),
        }
    }

    pub fn detailed(&self) -> VmaDetailedStatistics {
        let mut d = VmaDetailedStatistics::default();
        d.statistics = self.stats();
        d.unused_range_count = self.free.unused_range_count();
        for a in self.allocs.values() {
            d.allocation_size_min = d.allocation_size_min.min(a.size);
            d.allocation_size_max = d.allocation_size_max.max(a.size);
        }
        d
    }
}

pub unsafe fn create(info: *const VmaVirtualBlockCreateInfo, out: *mut VmaVirtualBlock) -> i32 {
    if info.is_null() || out.is_null() {
        return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    }
    let size = (*info).size;
    if size == 0 {
        return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    }
    let b = Box::new(VirtualBlock::new(size, (*info).flags));
    *out = Box::into_raw(b);
    VK_SUCCESS
}

pub unsafe fn destroy(block: VmaVirtualBlock) {
    if !block.is_null() {
        drop(Box::from_raw(block));
    }
}

pub unsafe fn is_empty(block: VmaVirtualBlock) -> u32 {
    if block.is_null() {
        return VK_TRUE;
    }
    if (*block).allocs.is_empty() {
        VK_TRUE
    } else {
        VK_FALSE
    }
}

pub unsafe fn allocate(
    block: VmaVirtualBlock,
    create: *const VmaVirtualAllocationCreateInfo,
    allocation: *mut VmaVirtualAllocation,
    offset: *mut u64,
) -> i32 {
    if block.is_null() || create.is_null() || allocation.is_null() {
        return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    }
    let c = *create;
    match (*block).allocate(c.size, c.alignment, c.flags, c.p_user_data) {
        Some((id, off)) => {
            *allocation = id;
            if !offset.is_null() {
                *offset = off;
            }
            VK_SUCCESS
        }
        None => {
            *allocation = 0;
            if !offset.is_null() {
                *offset = u64::MAX;
            }
            VK_ERROR_OUT_OF_DEVICE_MEMORY
        }
    }
}

pub unsafe fn free(block: VmaVirtualBlock, allocation: VmaVirtualAllocation) {
    if block.is_null() || allocation == 0 {
        return;
    }
    (*block).free(allocation);
}

pub unsafe fn get_info(
    block: VmaVirtualBlock,
    allocation: VmaVirtualAllocation,
    info: *mut VmaVirtualAllocationInfo,
) {
    if block.is_null() || info.is_null() {
        return;
    }
    if let Some(a) = (*block).allocs.get(&allocation) {
        *info = VmaVirtualAllocationInfo {
            offset: a.offset,
            size: a.size,
            p_user_data: a.user_data,
        };
    }
}

pub unsafe fn set_user_data(
    block: VmaVirtualBlock,
    allocation: VmaVirtualAllocation,
    user: *mut c_void,
) {
    if block.is_null() {
        return;
    }
    if let Some(a) = (*block).allocs.get_mut(&allocation) {
        a.user_data = user;
    }
}

pub unsafe fn stats_string(block: VmaVirtualBlock) -> *mut c_char {
    use std::ffi::CString;
    if block.is_null() {
        return core::ptr::null_mut();
    }
    let s = (*block).stats();
    let json = format!(
        "{{\"blockCount\":{},\"allocationCount\":{},\"blockBytes\":{},\"allocationBytes\":{}}}\0",
        s.block_count, s.allocation_count, s.block_bytes, s.allocation_bytes
    );
    CString::new(json.trim_end_matches('\0'))
        .ok()
        .map(|c| c.into_raw())
        .unwrap_or(core::ptr::null_mut())
}

pub unsafe fn free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(std::ffi::CString::from_raw(s));
    }
}

#[inline]
pub fn align_req(size: u64, align: u64) -> u64 {
    align_up(size, align.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vk::{VK_ERROR_OUT_OF_DEVICE_MEMORY, VK_SUCCESS, VK_TRUE};

    #[test]
    fn create_alloc_free() {
        let mut ci = VmaVirtualBlockCreateInfo {
            size: 1024,
            flags: 0,
            p_allocation_callbacks: core::ptr::null(),
        };
        let mut block = core::ptr::null_mut();
        unsafe {
            assert_eq!(create(&ci, &mut block), VK_SUCCESS);
            assert_eq!(is_empty(block), VK_TRUE);
            let aci = VmaVirtualAllocationCreateInfo {
                size: 64,
                alignment: 16,
                flags: 0x20000,
                p_user_data: core::ptr::null_mut(),
            };
            let mut id = 0;
            let mut off = 0;
            assert_eq!(allocate(block, &aci, &mut id, &mut off), VK_SUCCESS);
            assert_eq!(off % 16, 0);
            let mut id2 = 0;
            let mut off2 = 0;
            assert_eq!(allocate(block, &aci, &mut id2, &mut off2), VK_SUCCESS);
            assert_ne!(off, off2);
            free(block, id);
            free(block, id2);
            assert_eq!(is_empty(block), VK_TRUE);
            destroy(block);
            ci.size = 0;
            assert_eq!(create(&ci, &mut block), VK_ERROR_OUT_OF_DEVICE_MEMORY);
        }
    }
}
