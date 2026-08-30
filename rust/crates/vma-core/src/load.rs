//! Load `VmaVulkanFunctions` from the create-info table or `libvulkan.so.1`.

use crate::types::VmaVulkanFunctions;
use crate::vk::*;

pub fn resolve(
    instance: VkInstance,
    device: VkDevice,
    provided: Option<&VmaVulkanFunctions>,
) -> Result<VmaVulkanFunctions, VkResult> {
    let mut f = provided.copied().unwrap_or_default();
    if f.vk_get_instance_proc_addr.is_none() {
        f.vk_get_instance_proc_addr = load_gipa();
    }
    let gipa = f.vk_get_instance_proc_addr.ok_or(VK_ERROR_FEATURE_NOT_PRESENT)?;
    unsafe {
        if f.vk_get_device_proc_addr.is_none() {
            f.vk_get_device_proc_addr = core::mem::transmute(gipa(instance, c"vkGetDeviceProcAddr".as_ptr()));
        }
        let gdpa = f.vk_get_device_proc_addr;
        macro_rules! inst {
            ($field:ident, $name:expr) => {
                if f.$field.is_none() {
                    f.$field = core::mem::transmute(gipa(instance, $name.as_ptr()));
                }
            };
        }
        macro_rules! dev {
            ($field:ident, $name:expr) => {
                if f.$field.is_none() {
                    if let Some(g) = gdpa {
                        f.$field = core::mem::transmute(g(device, $name.as_ptr()));
                    }
                    if f.$field.is_none() {
                        f.$field = core::mem::transmute(gipa(instance, $name.as_ptr()));
                    }
                }
            };
        }
        inst!(vk_get_physical_device_properties, c"vkGetPhysicalDeviceProperties");
        inst!(
            vk_get_physical_device_memory_properties,
            c"vkGetPhysicalDeviceMemoryProperties"
        );
        inst!(
            vk_get_physical_device_memory_properties2_khr,
            c"vkGetPhysicalDeviceMemoryProperties2"
        );
        dev!(vk_allocate_memory, c"vkAllocateMemory");
        dev!(vk_free_memory, c"vkFreeMemory");
        dev!(vk_map_memory, c"vkMapMemory");
        dev!(vk_unmap_memory, c"vkUnmapMemory");
        dev!(vk_flush_mapped_memory_ranges, c"vkFlushMappedMemoryRanges");
        dev!(
            vk_invalidate_mapped_memory_ranges,
            c"vkInvalidateMappedMemoryRanges"
        );
        dev!(vk_bind_buffer_memory, c"vkBindBufferMemory");
        dev!(vk_bind_image_memory, c"vkBindImageMemory");
        dev!(vk_get_buffer_memory_requirements, c"vkGetBufferMemoryRequirements");
        dev!(vk_get_image_memory_requirements, c"vkGetImageMemoryRequirements");
        dev!(vk_create_buffer, c"vkCreateBuffer");
        dev!(vk_destroy_buffer, c"vkDestroyBuffer");
        dev!(vk_create_image, c"vkCreateImage");
        dev!(vk_destroy_image, c"vkDestroyImage");
        dev!(vk_get_buffer_memory_requirements2_khr, c"vkGetBufferMemoryRequirements2");
        if f.vk_get_buffer_memory_requirements2_khr.is_none() {
            dev!(vk_get_buffer_memory_requirements2_khr, c"vkGetBufferMemoryRequirements2KHR");
        }
        dev!(vk_get_image_memory_requirements2_khr, c"vkGetImageMemoryRequirements2");
        if f.vk_get_image_memory_requirements2_khr.is_none() {
            dev!(vk_get_image_memory_requirements2_khr, c"vkGetImageMemoryRequirements2KHR");
        }
        dev!(vk_bind_buffer_memory2_khr, c"vkBindBufferMemory2");
        if f.vk_bind_buffer_memory2_khr.is_none() {
            dev!(vk_bind_buffer_memory2_khr, c"vkBindBufferMemory2KHR");
        }
        dev!(vk_bind_image_memory2_khr, c"vkBindImageMemory2");
        if f.vk_bind_image_memory2_khr.is_none() {
            dev!(vk_bind_image_memory2_khr, c"vkBindImageMemory2KHR");
        }
        dev!(vk_get_device_buffer_memory_requirements, c"vkGetDeviceBufferMemoryRequirements");
        dev!(vk_get_device_image_memory_requirements, c"vkGetDeviceImageMemoryRequirements");
    }
    if f.vk_get_physical_device_properties.is_none()
        || f.vk_get_physical_device_memory_properties.is_none()
        || f.vk_allocate_memory.is_none()
        || f.vk_free_memory.is_none()
        || f.vk_map_memory.is_none()
        || f.vk_create_buffer.is_none()
    {
        return Err(VK_ERROR_FEATURE_NOT_PRESENT);
    }
    Ok(f)
}

fn load_gipa() -> PFN_vkGetInstanceProcAddr {
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        let lib = libc::dlopen(c"libvulkan.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if lib.is_null() {
            let lib = libc::dlopen(c"libvulkan.so".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if lib.is_null() {
                return None;
            }
            return core::mem::transmute(libc::dlsym(lib, c"vkGetInstanceProcAddr".as_ptr()));
        }
        core::mem::transmute(libc::dlsym(lib, c"vkGetInstanceProcAddr".as_ptr()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}
