//! Vulkan types matching `vulkan_core.h` (64-bit Linux ABI).
//!
//! This is the subset VMA 3.4 needs, not a full Vulkan binding. Dispatchable
//! handles are pointers; non-dispatchable handles are `u64`.

use core::ffi::{c_char, c_void};

pub type VkFlags = u32;
pub type VkBool32 = u32;
pub type VkDeviceSize = u64;
pub type VkResult = i32;
pub type VkStructureType = i32;
pub type VkFlags64 = u64;

pub type VkInstance = *mut c_void;
pub type VkPhysicalDevice = *mut c_void;
pub type VkDevice = *mut c_void;
pub type VkQueue = *mut c_void;
pub type VkCommandBuffer = *mut c_void;
pub type VkDeviceMemory = u64;
pub type VkBuffer = u64;
pub type VkImage = u64;

pub const VK_SUCCESS: VkResult = 0;
pub const VK_INCOMPLETE: VkResult = 5;
pub const VK_ERROR_OUT_OF_HOST_MEMORY: VkResult = -1;
pub const VK_ERROR_OUT_OF_DEVICE_MEMORY: VkResult = -2;
pub const VK_ERROR_FEATURE_NOT_PRESENT: VkResult = -8;
pub const VK_ERROR_UNKNOWN: VkResult = -13;

pub const VK_FALSE: VkBool32 = 0;
pub const VK_TRUE: VkBool32 = 1;
pub const VK_WHOLE_SIZE: VkDeviceSize = !0;
pub const VK_NULL_HANDLE: u64 = 0;

pub const VK_MAX_MEMORY_TYPES: usize = 32;
pub const VK_MAX_MEMORY_HEAPS: usize = 16;
pub const VK_MAX_PHYSICAL_DEVICE_NAME_SIZE: usize = 256;
pub const VK_UUID_SIZE: usize = 16;

pub const VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT: u32 = 0x1;
pub const VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT: u32 = 0x2;
pub const VK_MEMORY_PROPERTY_HOST_COHERENT_BIT: u32 = 0x4;
pub const VK_MEMORY_PROPERTY_HOST_CACHED_BIT: u32 = 0x8;
pub const VK_MEMORY_PROPERTY_LAZILY_ALLOCATED_BIT: u32 = 0x10;
pub const VK_MEMORY_PROPERTY_DEVICE_COHERENT_BIT_AMD: u32 = 0x40;

pub const VK_IMAGE_TILING_OPTIMAL: u32 = 0;
pub const VK_IMAGE_TILING_LINEAR: u32 = 1;
pub const VK_BUFFER_USAGE_TRANSFER_SRC_BIT: u32 = 0x00000001;
pub const VK_BUFFER_USAGE_TRANSFER_DST_BIT: u32 = 0x00000002;
pub const VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT: u32 = 0x00000010;
pub const VK_BUFFER_USAGE_INDEX_BUFFER_BIT: u32 = 0x00000040;
pub const VK_BUFFER_USAGE_VERTEX_BUFFER_BIT: u32 = 0x00000080;
pub const VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT: u32 = 0x00020000;
pub const VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT: u32 = 0x2;
pub const VK_IMAGE_USAGE_TRANSFER_DST_BIT: u32 = 0x00000002;
pub const VK_IMAGE_USAGE_SAMPLED_BIT: u32 = 0x00000004;
pub const VK_API_VERSION_1_2: u32 = (1 << 22) | (2 << 12);

pub const VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2: i32 = 1000059001;
pub const VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO: i32 = 5;
pub const VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE: i32 = 6;
pub const VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO: i32 = 12;
pub const VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO: i32 = 14;
pub const VK_STRUCTURE_TYPE_BIND_BUFFER_MEMORY_INFO: i32 = 1000157000;
pub const VK_STRUCTURE_TYPE_BIND_IMAGE_MEMORY_INFO: i32 = 1000157001;
pub const VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO: i32 = 1000127001;
pub const VK_STRUCTURE_TYPE_BUFFER_MEMORY_REQUIREMENTS_INFO_2: i32 = 1000146000;
pub const VK_STRUCTURE_TYPE_IMAGE_MEMORY_REQUIREMENTS_INFO_2: i32 = 1000146001;
pub const VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_FLAGS_INFO: i32 = 1000060000;
pub const VK_STRUCTURE_TYPE_DEVICE_BUFFER_MEMORY_REQUIREMENTS: i32 = 1000413002;
pub const VK_STRUCTURE_TYPE_DEVICE_IMAGE_MEMORY_REQUIREMENTS: i32 = 1000413003;
pub const VK_STRUCTURE_TYPE_MEMORY_REQUIREMENTS_2: i32 = 1000146003;

pub type PfnVoid = Option<unsafe extern "system" fn()>;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkExtent3D {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkMemoryHeap {
    pub size: VkDeviceSize,
    pub flags: VkFlags,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkMemoryType {
    pub property_flags: VkFlags,
    pub heap_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPhysicalDeviceMemoryProperties {
    pub memory_type_count: u32,
    pub memory_types: [VkMemoryType; VK_MAX_MEMORY_TYPES],
    pub memory_heap_count: u32,
    pub memory_heaps: [VkMemoryHeap; VK_MAX_MEMORY_HEAPS],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPhysicalDeviceLimits {
    pub max_image_dimension_1d: u32,
    pub max_image_dimension_2d: u32,
    pub max_image_dimension_3d: u32,
    pub max_image_dimension_cube: u32,
    pub max_image_array_layers: u32,
    pub max_texel_buffer_elements: u32,
    pub max_uniform_buffer_range: u32,
    pub max_storage_buffer_range: u32,
    pub max_push_constants_size: u32,
    pub max_memory_allocation_count: u32,
    pub max_sampler_allocation_count: u32,
    pub buffer_image_granularity: VkDeviceSize,
    pub sparse_address_space_size: VkDeviceSize,
    pub max_bound_descriptor_sets: u32,
    pub max_per_stage_descriptor_samplers: u32,
    pub max_per_stage_descriptor_uniform_buffers: u32,
    pub max_per_stage_descriptor_storage_buffers: u32,
    pub max_per_stage_descriptor_sampled_images: u32,
    pub max_per_stage_descriptor_storage_images: u32,
    pub max_per_stage_descriptor_input_attachments: u32,
    pub max_per_stage_resources: u32,
    pub max_descriptor_set_samplers: u32,
    pub max_descriptor_set_uniform_buffers: u32,
    pub max_descriptor_set_uniform_buffers_dynamic: u32,
    pub max_descriptor_set_storage_buffers: u32,
    pub max_descriptor_set_storage_buffers_dynamic: u32,
    pub max_descriptor_set_sampled_images: u32,
    pub max_descriptor_set_storage_images: u32,
    pub max_descriptor_set_input_attachments: u32,
    pub max_vertex_input_attributes: u32,
    pub max_vertex_input_bindings: u32,
    pub max_vertex_input_attribute_offset: u32,
    pub max_vertex_input_binding_stride: u32,
    pub max_vertex_output_components: u32,
    pub max_tessellation_generation_level: u32,
    pub max_tessellation_patch_size: u32,
    pub max_tessellation_control_per_vertex_input_components: u32,
    pub max_tessellation_control_per_vertex_output_components: u32,
    pub max_tessellation_control_per_patch_output_components: u32,
    pub max_tessellation_control_total_output_components: u32,
    pub max_tessellation_evaluation_input_components: u32,
    pub max_tessellation_evaluation_output_components: u32,
    pub max_geometry_shader_invocations: u32,
    pub max_geometry_input_components: u32,
    pub max_geometry_output_components: u32,
    pub max_geometry_output_vertices: u32,
    pub max_geometry_total_output_components: u32,
    pub max_fragment_input_components: u32,
    pub max_fragment_output_attachments: u32,
    pub max_fragment_dual_src_attachments: u32,
    pub max_fragment_combined_output_resources: u32,
    pub max_compute_shared_memory_size: u32,
    pub max_compute_work_group_count: [u32; 3],
    pub max_compute_work_group_invocations: u32,
    pub max_compute_work_group_size: [u32; 3],
    pub sub_pixel_precision_bits: u32,
    pub sub_texel_precision_bits: u32,
    pub mipmap_precision_bits: u32,
    pub max_draw_indexed_index_value: u32,
    pub max_draw_indirect_count: u32,
    pub max_sampler_lod_bias: f32,
    pub max_sampler_anisotropy: f32,
    pub max_viewports: u32,
    pub max_viewport_dimensions: [u32; 2],
    pub viewport_bounds_range: [f32; 2],
    pub viewport_sub_pixel_bits: u32,
    pub min_memory_map_alignment: usize,
    pub min_texel_buffer_offset_alignment: VkDeviceSize,
    pub min_uniform_buffer_offset_alignment: VkDeviceSize,
    pub min_storage_buffer_offset_alignment: VkDeviceSize,
    pub min_texel_offset: i32,
    pub max_texel_offset: u32,
    pub min_texel_gather_offset: i32,
    pub max_texel_gather_offset: u32,
    pub min_interpolation_offset: f32,
    pub max_interpolation_offset: f32,
    pub sub_pixel_interpolation_offset_bits: u32,
    pub max_framebuffer_width: u32,
    pub max_framebuffer_height: u32,
    pub max_framebuffer_layers: u32,
    pub framebuffer_color_sample_counts: VkFlags,
    pub framebuffer_depth_sample_counts: VkFlags,
    pub framebuffer_stencil_sample_counts: VkFlags,
    pub framebuffer_no_attachments_sample_counts: VkFlags,
    pub max_color_attachments: u32,
    pub sampled_image_color_sample_counts: VkFlags,
    pub sampled_image_integer_sample_counts: VkFlags,
    pub sampled_image_depth_sample_counts: VkFlags,
    pub sampled_image_stencil_sample_counts: VkFlags,
    pub storage_image_sample_counts: VkFlags,
    pub max_sample_mask_words: u32,
    pub timestamp_compute_and_graphics: VkBool32,
    pub timestamp_period: f32,
    pub max_clip_distances: u32,
    pub max_cull_distances: u32,
    pub max_combined_clip_and_cull_distances: u32,
    pub discrete_queue_priorities: u32,
    pub point_size_range: [f32; 2],
    pub line_width_range: [f32; 2],
    pub point_size_granularity: f32,
    pub line_width_granularity: f32,
    pub strict_lines: VkBool32,
    pub standard_sample_locations: VkBool32,
    pub optimal_buffer_copy_offset_alignment: VkDeviceSize,
    pub optimal_buffer_copy_row_pitch_alignment: VkDeviceSize,
    pub non_coherent_atom_size: VkDeviceSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPhysicalDeviceSparseProperties {
    pub residency_standard_2d_block_shape: VkBool32,
    pub residency_standard_2d_multisample_block_shape: VkBool32,
    pub residency_standard_3d_block_shape: VkBool32,
    pub residency_aligned_mip_size: VkBool32,
    pub residency_non_resident_strict: VkBool32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPhysicalDeviceProperties {
    pub api_version: u32,
    pub driver_version: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: u32,
    pub device_name: [c_char; VK_MAX_PHYSICAL_DEVICE_NAME_SIZE],
    pub pipeline_cache_uuid: [u8; VK_UUID_SIZE],
    pub limits: VkPhysicalDeviceLimits,
    pub sparse_properties: VkPhysicalDeviceSparseProperties,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPhysicalDeviceProperties2 {
    pub s_type: VkStructureType,
    pub p_next: *mut c_void,
    pub properties: VkPhysicalDeviceProperties,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkMemoryRequirements {
    pub size: VkDeviceSize,
    pub alignment: VkDeviceSize,
    pub memory_type_bits: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkMemoryAllocateInfo {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub allocation_size: VkDeviceSize,
    pub memory_type_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkMappedMemoryRange {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub memory: VkDeviceMemory,
    pub offset: VkDeviceSize,
    pub size: VkDeviceSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkBufferCreateInfo {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub size: VkDeviceSize,
    pub usage: VkFlags,
    pub sharing_mode: u32,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkImageCreateInfo {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub image_type: u32,
    pub format: u32,
    pub extent: VkExtent3D,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub samples: u32,
    pub tiling: u32,
    pub usage: VkFlags,
    pub sharing_mode: u32,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
    pub initial_layout: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkAllocationCallbacks {
    pub p_user_data: *mut c_void,
    pub pfn_allocation: PfnVoid,
    pub pfn_reallocation: PfnVoid,
    pub pfn_free: PfnVoid,
    pub pfn_internal_allocation: PfnVoid,
    pub pfn_internal_free: PfnVoid,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkBindBufferMemoryInfo {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub buffer: VkBuffer,
    pub memory: VkDeviceMemory,
    pub memory_offset: VkDeviceSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkBindImageMemoryInfo {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub image: VkImage,
    pub memory: VkDeviceMemory,
    pub memory_offset: VkDeviceSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkMemoryDedicatedAllocateInfo {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub image: VkImage,
    pub buffer: VkBuffer,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkMemoryAllocateFlagsInfo {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub flags: VkFlags,
    pub device_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkBufferMemoryRequirementsInfo2 {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub buffer: VkBuffer,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkImageMemoryRequirementsInfo2 {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub image: VkImage,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkMemoryRequirements2 {
    pub s_type: VkStructureType,
    pub p_next: *mut c_void,
    pub memory_requirements: VkMemoryRequirements,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkDeviceBufferMemoryRequirements {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub p_create_info: *const VkBufferCreateInfo,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkDeviceImageMemoryRequirements {
    pub s_type: VkStructureType,
    pub p_next: *const c_void,
    pub p_create_info: *const VkImageCreateInfo,
    pub plane_aspect: u32,
}

pub type PFN_vkVoidFunction = Option<unsafe extern "system" fn()>;
pub type PFN_vkGetInstanceProcAddr =
    Option<unsafe extern "system" fn(VkInstance, *const c_char) -> PFN_vkVoidFunction>;
pub type PFN_vkGetDeviceProcAddr =
    Option<unsafe extern "system" fn(VkDevice, *const c_char) -> PFN_vkVoidFunction>;
pub type PFN_vkGetPhysicalDeviceProperties =
    Option<unsafe extern "system" fn(VkPhysicalDevice, *mut VkPhysicalDeviceProperties)>;
pub type PFN_vkGetPhysicalDeviceProperties2 =
    Option<unsafe extern "system" fn(VkPhysicalDevice, *mut VkPhysicalDeviceProperties2)>;
pub type PFN_vkGetPhysicalDeviceMemoryProperties =
    Option<unsafe extern "system" fn(VkPhysicalDevice, *mut VkPhysicalDeviceMemoryProperties)>;
pub type PFN_vkAllocateMemory = Option<
    unsafe extern "system" fn(
        VkDevice,
        *const VkMemoryAllocateInfo,
        *const VkAllocationCallbacks,
        *mut VkDeviceMemory,
    ) -> VkResult,
>;
pub type PFN_vkFreeMemory =
    Option<unsafe extern "system" fn(VkDevice, VkDeviceMemory, *const VkAllocationCallbacks)>;
pub type PFN_vkMapMemory = Option<
    unsafe extern "system" fn(
        VkDevice,
        VkDeviceMemory,
        VkDeviceSize,
        VkDeviceSize,
        VkFlags,
        *mut *mut c_void,
    ) -> VkResult,
>;
pub type PFN_vkUnmapMemory = Option<unsafe extern "system" fn(VkDevice, VkDeviceMemory)>;
pub type PFN_vkFlushMappedMemoryRanges =
    Option<unsafe extern "system" fn(VkDevice, u32, *const VkMappedMemoryRange) -> VkResult>;
pub type PFN_vkInvalidateMappedMemoryRanges =
    Option<unsafe extern "system" fn(VkDevice, u32, *const VkMappedMemoryRange) -> VkResult>;
pub type PFN_vkBindBufferMemory =
    Option<unsafe extern "system" fn(VkDevice, VkBuffer, VkDeviceMemory, VkDeviceSize) -> VkResult>;
pub type PFN_vkBindImageMemory =
    Option<unsafe extern "system" fn(VkDevice, VkImage, VkDeviceMemory, VkDeviceSize) -> VkResult>;
pub type PFN_vkGetBufferMemoryRequirements =
    Option<unsafe extern "system" fn(VkDevice, VkBuffer, *mut VkMemoryRequirements)>;
pub type PFN_vkGetImageMemoryRequirements =
    Option<unsafe extern "system" fn(VkDevice, VkImage, *mut VkMemoryRequirements)>;
pub type PFN_vkCreateBuffer = Option<
    unsafe extern "system" fn(
        VkDevice,
        *const VkBufferCreateInfo,
        *const VkAllocationCallbacks,
        *mut VkBuffer,
    ) -> VkResult,
>;
pub type PFN_vkDestroyBuffer =
    Option<unsafe extern "system" fn(VkDevice, VkBuffer, *const VkAllocationCallbacks)>;
pub type PFN_vkCreateImage = Option<
    unsafe extern "system" fn(
        VkDevice,
        *const VkImageCreateInfo,
        *const VkAllocationCallbacks,
        *mut VkImage,
    ) -> VkResult,
>;
pub type PFN_vkDestroyImage =
    Option<unsafe extern "system" fn(VkDevice, VkImage, *const VkAllocationCallbacks)>;
pub type PFN_vkCmdCopyBuffer = Option<unsafe extern "system" fn()>;
pub type PFN_vkGetBufferMemoryRequirements2 = Option<
    unsafe extern "system" fn(
        VkDevice,
        *const VkBufferMemoryRequirementsInfo2,
        *mut VkMemoryRequirements2,
    ),
>;
pub type PFN_vkGetImageMemoryRequirements2 = Option<
    unsafe extern "system" fn(
        VkDevice,
        *const VkImageMemoryRequirementsInfo2,
        *mut VkMemoryRequirements2,
    ),
>;
pub type PFN_vkBindBufferMemory2 =
    Option<unsafe extern "system" fn(VkDevice, u32, *const VkBindBufferMemoryInfo) -> VkResult>;
pub type PFN_vkBindImageMemory2 =
    Option<unsafe extern "system" fn(VkDevice, u32, *const VkBindImageMemoryInfo) -> VkResult>;
pub type PFN_vkGetPhysicalDeviceMemoryProperties2 = Option<unsafe extern "system" fn()>;
pub type PFN_vkGetDeviceBufferMemoryRequirements = Option<
    unsafe extern "system" fn(
        VkDevice,
        *const VkDeviceBufferMemoryRequirements,
        *mut VkMemoryRequirements2,
    ),
>;
pub type PFN_vkGetDeviceImageMemoryRequirements = Option<
    unsafe extern "system" fn(
        VkDevice,
        *const VkDeviceImageMemoryRequirements,
        *mut VkMemoryRequirements2,
    ),
>;
