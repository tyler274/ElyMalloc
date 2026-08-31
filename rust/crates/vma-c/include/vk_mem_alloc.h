#ifndef AMD_VULKAN_MEMORY_ALLOCATOR_H
#define AMD_VULKAN_MEMORY_ALLOCATOR_H

/*
 * Declarations-only header matching AMD Vulkan Memory Allocator 3.4
 * (`vk_mem_alloc.h` v3.4.0) for linking the Rust `libVulkanMemoryAllocator`.
 * Do not define VMA_IMPLEMENTATION. Apps may include AMD's header instead.
 */

#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>
#include <stdint.h>

#ifndef VK_VERSION_1_0
typedef uint32_t VkFlags;
typedef uint32_t VkBool32;
typedef uint64_t VkDeviceSize;
typedef int32_t VkResult;
typedef int32_t VkStructureType;
typedef VkFlags VkMemoryPropertyFlags;

typedef struct VkInstance_T *VkInstance;
typedef struct VkPhysicalDevice_T *VkPhysicalDevice;
typedef struct VkDevice_T *VkDevice;
typedef uint64_t VkDeviceMemory;
typedef uint64_t VkBuffer;
typedef uint64_t VkImage;

#ifndef VK_DEFINE_HANDLE
#define VK_DEFINE_HANDLE(object) typedef struct object##_T *object;
#endif

#define VK_NULL_HANDLE 0
#define VK_WHOLE_SIZE (~(VkDeviceSize)0)
#define VK_TRUE 1
#define VK_FALSE 0
#define VK_SUCCESS 0
#define VK_INCOMPLETE 5
#define VK_ERROR_OUT_OF_HOST_MEMORY (-1)
#define VK_ERROR_OUT_OF_DEVICE_MEMORY (-2)
#define VK_ERROR_FEATURE_NOT_PRESENT (-8)
#define VK_ERROR_UNKNOWN (-13)
#define VK_MAX_MEMORY_TYPES 32
#define VK_MAX_MEMORY_HEAPS 16
#define VK_MAX_PHYSICAL_DEVICE_NAME_SIZE 256
#define VK_UUID_SIZE 16
#ifndef VK_MAKE_VERSION
#define VK_MAKE_VERSION(major, minor, patch) \
    ((((uint32_t)(major)) << 22) | (((uint32_t)(minor)) << 12) | ((uint32_t)(patch)))
#endif
#define VMA_VERSION (VK_MAKE_VERSION(3, 4, 0))

typedef struct VkExtent3D {
    uint32_t width, height, depth;
} VkExtent3D;
typedef struct VkMemoryHeap {
    VkDeviceSize size;
    VkFlags flags;
} VkMemoryHeap;
typedef struct VkMemoryType {
    VkFlags propertyFlags;
    uint32_t heapIndex;
} VkMemoryType;
typedef struct VkPhysicalDeviceMemoryProperties {
    uint32_t memoryTypeCount;
    VkMemoryType memoryTypes[VK_MAX_MEMORY_TYPES];
    uint32_t memoryHeapCount;
    VkMemoryHeap memoryHeaps[VK_MAX_MEMORY_HEAPS];
} VkPhysicalDeviceMemoryProperties;
typedef struct VkPhysicalDeviceLimits {
    uint32_t maxImageDimension1D, maxImageDimension2D, maxImageDimension3D, maxImageDimensionCube;
    uint32_t maxImageArrayLayers, maxTexelBufferElements, maxUniformBufferRange, maxStorageBufferRange;
    uint32_t maxPushConstantsSize, maxMemoryAllocationCount, maxSamplerAllocationCount;
    VkDeviceSize bufferImageGranularity, sparseAddressSpaceSize;
    uint32_t maxBoundDescriptorSets;
    uint32_t maxPerStageDescriptorSamplers, maxPerStageDescriptorUniformBuffers;
    uint32_t maxPerStageDescriptorStorageBuffers, maxPerStageDescriptorSampledImages;
    uint32_t maxPerStageDescriptorStorageImages, maxPerStageDescriptorInputAttachments;
    uint32_t maxPerStageResources;
    uint32_t maxDescriptorSetSamplers, maxDescriptorSetUniformBuffers;
    uint32_t maxDescriptorSetUniformBuffersDynamic, maxDescriptorSetStorageBuffers;
    uint32_t maxDescriptorSetStorageBuffersDynamic, maxDescriptorSetSampledImages;
    uint32_t maxDescriptorSetStorageImages, maxDescriptorSetInputAttachments;
    uint32_t maxVertexInputAttributes, maxVertexInputBindings, maxVertexInputAttributeOffset;
    uint32_t maxVertexInputBindingStride, maxVertexOutputComponents;
    uint32_t maxTessellationGenerationLevel, maxTessellationPatchSize;
    uint32_t maxTessellationControlPerVertexInputComponents;
    uint32_t maxTessellationControlPerVertexOutputComponents;
    uint32_t maxTessellationControlPerPatchOutputComponents;
    uint32_t maxTessellationControlTotalOutputComponents;
    uint32_t maxTessellationEvaluationInputComponents, maxTessellationEvaluationOutputComponents;
    uint32_t maxGeometryShaderInvocations, maxGeometryInputComponents, maxGeometryOutputComponents;
    uint32_t maxGeometryOutputVertices, maxGeometryTotalOutputComponents;
    uint32_t maxFragmentInputComponents, maxFragmentOutputAttachments, maxFragmentDualSrcAttachments;
    uint32_t maxFragmentCombinedOutputResources, maxComputeSharedMemorySize;
    uint32_t maxComputeWorkGroupCount[3], maxComputeWorkGroupInvocations, maxComputeWorkGroupSize[3];
    uint32_t subPixelPrecisionBits, subTexelPrecisionBits, mipmapPrecisionBits;
    uint32_t maxDrawIndexedIndexValue, maxDrawIndirectCount;
    float maxSamplerLodBias, maxSamplerAnisotropy;
    uint32_t maxViewports, maxViewportDimensions[2];
    float viewportBoundsRange[2];
    uint32_t viewportSubPixelBits;
    size_t minMemoryMapAlignment;
    VkDeviceSize minTexelBufferOffsetAlignment, minUniformBufferOffsetAlignment, minStorageBufferOffsetAlignment;
    int32_t minTexelOffset;
    uint32_t maxTexelOffset;
    int32_t minTexelGatherOffset;
    uint32_t maxTexelGatherOffset;
    float minInterpolationOffset, maxInterpolationOffset;
    uint32_t subPixelInterpolationOffsetBits, maxFramebufferWidth, maxFramebufferHeight, maxFramebufferLayers;
    VkFlags framebufferColorSampleCounts, framebufferDepthSampleCounts, framebufferStencilSampleCounts;
    VkFlags framebufferNoAttachmentsSampleCounts;
    uint32_t maxColorAttachments;
    VkFlags sampledImageColorSampleCounts, sampledImageIntegerSampleCounts;
    VkFlags sampledImageDepthSampleCounts, sampledImageStencilSampleCounts, storageImageSampleCounts;
    uint32_t maxSampleMaskWords;
    VkBool32 timestampComputeAndGraphics;
    float timestampPeriod;
    uint32_t maxClipDistances, maxCullDistances, maxCombinedClipAndCullDistances, discreteQueuePriorities;
    float pointSizeRange[2], lineWidthRange[2], pointSizeGranularity, lineWidthGranularity;
    VkBool32 strictLines, standardSampleLocations;
    VkDeviceSize optimalBufferCopyOffsetAlignment, optimalBufferCopyRowPitchAlignment, nonCoherentAtomSize;
} VkPhysicalDeviceLimits;
typedef struct VkPhysicalDeviceSparseProperties {
    VkBool32 residencyStandard2DBlockShape, residencyStandard2DMultisampleBlockShape;
    VkBool32 residencyStandard3DBlockShape, residencyAlignedMipSize, residencyNonResidentStrict;
} VkPhysicalDeviceSparseProperties;
typedef struct VkPhysicalDeviceProperties {
    uint32_t apiVersion, driverVersion, vendorID, deviceID, deviceType;
    char deviceName[VK_MAX_PHYSICAL_DEVICE_NAME_SIZE];
    uint8_t pipelineCacheUUID[VK_UUID_SIZE];
    VkPhysicalDeviceLimits limits;
    VkPhysicalDeviceSparseProperties sparseProperties;
} VkPhysicalDeviceProperties;
typedef struct VkMemoryRequirements {
    VkDeviceSize size, alignment;
    uint32_t memoryTypeBits;
} VkMemoryRequirements;
typedef struct VkBufferCreateInfo {
    VkStructureType sType;
    const void *pNext;
    VkFlags flags;
    VkDeviceSize size;
    VkFlags usage;
    uint32_t sharingMode, queueFamilyIndexCount;
    const uint32_t *pQueueFamilyIndices;
} VkBufferCreateInfo;
typedef struct VkImageCreateInfo {
    VkStructureType sType;
    const void *pNext;
    VkFlags flags;
    uint32_t imageType, format;
    VkExtent3D extent;
    uint32_t mipLevels, arrayLayers, samples, tiling;
    VkFlags usage;
    uint32_t sharingMode, queueFamilyIndexCount;
    const uint32_t *pQueueFamilyIndices;
    uint32_t initialLayout;
} VkImageCreateInfo;
typedef struct VkAllocationCallbacks {
    void *pUserData;
    void *pfnAllocation, *pfnReallocation, *pfnFree, *pfnInternalAllocation, *pfnInternalFree;
} VkAllocationCallbacks;
#endif /* VK_VERSION_1_0 */

#ifndef VMA_NULLABLE
#define VMA_NULLABLE
#endif
#ifndef VMA_NOT_NULL
#define VMA_NOT_NULL
#endif
#ifndef VMA_NULLABLE_NON_DISPATCHABLE
#define VMA_NULLABLE_NON_DISPATCHABLE
#endif
#ifndef VMA_NOT_NULL_NON_DISPATCHABLE
#define VMA_NOT_NULL_NON_DISPATCHABLE
#endif
#ifndef VMA_CALL_PRE
#define VMA_CALL_PRE
#endif
#ifndef VMA_CALL_POST
#define VMA_CALL_POST
#endif

typedef struct VmaAllocator_T *VmaAllocator;
typedef struct VmaPool_T *VmaPool;
typedef struct VmaAllocation_T *VmaAllocation;
typedef struct VmaDefragmentationContext_T *VmaDefragmentationContext;
typedef struct VmaVirtualBlock_T *VmaVirtualBlock;
typedef VkDeviceSize VmaVirtualAllocation;

#define VMA_ALLOCATOR_CREATE_EXTERNALLY_SYNCHRONIZED_BIT 0x00000001u
#define VMA_ALLOCATOR_CREATE_KHR_DEDICATED_ALLOCATION_BIT 0x00000002u
#define VMA_ALLOCATOR_CREATE_KHR_BIND_MEMORY2_BIT 0x00000004u
#define VMA_ALLOCATOR_CREATE_EXT_MEMORY_BUDGET_BIT 0x00000008u
#define VMA_ALLOCATOR_CREATE_AMD_DEVICE_COHERENT_MEMORY_BIT 0x00000010u
#define VMA_ALLOCATOR_CREATE_BUFFER_DEVICE_ADDRESS_BIT 0x00000020u
#define VMA_ALLOCATOR_CREATE_EXT_MEMORY_PRIORITY_BIT 0x00000040u
#define VMA_ALLOCATOR_CREATE_KHR_MAINTENANCE4_BIT 0x00000080u
#define VMA_ALLOCATOR_CREATE_KHR_MAINTENANCE5_BIT 0x00000100u
#define VMA_ALLOCATOR_CREATE_KHR_EXTERNAL_MEMORY_WIN32_BIT 0x00000200u

typedef enum VmaMemoryUsage {
    VMA_MEMORY_USAGE_UNKNOWN = 0,
    VMA_MEMORY_USAGE_GPU_ONLY = 1,
    VMA_MEMORY_USAGE_CPU_ONLY = 2,
    VMA_MEMORY_USAGE_CPU_TO_GPU = 3,
    VMA_MEMORY_USAGE_GPU_TO_CPU = 4,
    VMA_MEMORY_USAGE_CPU_COPY = 5,
    VMA_MEMORY_USAGE_GPU_LAZILY_ALLOCATED = 6,
    VMA_MEMORY_USAGE_AUTO = 7,
    VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE = 8,
    VMA_MEMORY_USAGE_AUTO_PREFER_HOST = 9,
    VMA_MEMORY_USAGE_MAX_ENUM = 0x7FFFFFFF
} VmaMemoryUsage;

#define VMA_ALLOCATION_CREATE_DEDICATED_MEMORY_BIT 0x00000001u
#define VMA_ALLOCATION_CREATE_NEVER_ALLOCATE_BIT 0x00000002u
#define VMA_ALLOCATION_CREATE_MAPPED_BIT 0x00000004u
#define VMA_ALLOCATION_CREATE_USER_DATA_COPY_STRING_BIT 0x00000020u
#define VMA_ALLOCATION_CREATE_UPPER_ADDRESS_BIT 0x00000040u
#define VMA_ALLOCATION_CREATE_DONT_BIND_BIT 0x00000080u
#define VMA_ALLOCATION_CREATE_WITHIN_BUDGET_BIT 0x00000100u
#define VMA_ALLOCATION_CREATE_CAN_ALIAS_BIT 0x00000200u
#define VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT 0x00000400u
#define VMA_ALLOCATION_CREATE_HOST_ACCESS_RANDOM_BIT 0x00000800u
#define VMA_ALLOCATION_CREATE_HOST_ACCESS_ALLOW_TRANSFER_INSTEAD_BIT 0x00001000u
#define VMA_ALLOCATION_CREATE_STRATEGY_MIN_MEMORY_BIT 0x00010000u
#define VMA_ALLOCATION_CREATE_STRATEGY_MIN_TIME_BIT 0x00020000u
#define VMA_ALLOCATION_CREATE_STRATEGY_MIN_OFFSET_BIT 0x00040000u
#define VMA_ALLOCATION_CREATE_STRATEGY_BEST_FIT_BIT VMA_ALLOCATION_CREATE_STRATEGY_MIN_MEMORY_BIT
#define VMA_ALLOCATION_CREATE_STRATEGY_FIRST_FIT_BIT VMA_ALLOCATION_CREATE_STRATEGY_MIN_TIME_BIT
#define VMA_ALLOCATION_CREATE_STRATEGY_MASK 0x00070000u

#define VMA_POOL_CREATE_IGNORE_BUFFER_IMAGE_GRANULARITY_BIT 0x00000002u
#define VMA_POOL_CREATE_LINEAR_ALGORITHM_BIT 0x00000004u
#define VMA_VIRTUAL_BLOCK_CREATE_LINEAR_ALGORITHM_BIT 0x00000001u

#ifndef VKAPI_PTR
#ifdef _WIN32
#define VKAPI_PTR __stdcall
#else
#define VKAPI_PTR
#endif
#endif
#ifndef PFN_vkVoidFunction
typedef void(VKAPI_PTR *PFN_vkVoidFunction)(void);
#endif
#ifndef PFN_vkGetInstanceProcAddr
typedef PFN_vkVoidFunction(VKAPI_PTR *PFN_vkGetInstanceProcAddr)(VkInstance, const char *);
typedef PFN_vkVoidFunction(VKAPI_PTR *PFN_vkGetDeviceProcAddr)(VkDevice, const char *);
#endif

typedef struct VmaDeviceMemoryCallbacks {
    void (*pfnAllocate)(VmaAllocator, uint32_t, VkDeviceMemory, VkDeviceSize, void *);
    void (*pfnFree)(VmaAllocator, uint32_t, VkDeviceMemory, VkDeviceSize, void *);
    void *pUserData;
} VmaDeviceMemoryCallbacks;

typedef struct VmaVulkanFunctions {
    PFN_vkGetInstanceProcAddr vkGetInstanceProcAddr;
    PFN_vkGetDeviceProcAddr vkGetDeviceProcAddr;
    void *vkGetPhysicalDeviceProperties;
    void *vkGetPhysicalDeviceMemoryProperties;
    void *vkAllocateMemory;
    void *vkFreeMemory;
    void *vkMapMemory;
    void *vkUnmapMemory;
    void *vkFlushMappedMemoryRanges;
    void *vkInvalidateMappedMemoryRanges;
    void *vkBindBufferMemory;
    void *vkBindImageMemory;
    void *vkGetBufferMemoryRequirements;
    void *vkGetImageMemoryRequirements;
    void *vkCreateBuffer;
    void *vkDestroyBuffer;
    void *vkCreateImage;
    void *vkDestroyImage;
    void *vkCmdCopyBuffer;
    void *vkGetBufferMemoryRequirements2KHR;
    void *vkGetImageMemoryRequirements2KHR;
    void *vkBindBufferMemory2KHR;
    void *vkBindImageMemory2KHR;
    void *vkGetPhysicalDeviceMemoryProperties2KHR;
    void *vkGetDeviceBufferMemoryRequirements;
    void *vkGetDeviceImageMemoryRequirements;
    void *vkGetMemoryWin32HandleKHR;
    void *vkGetPhysicalDeviceProperties2KHR;
} VmaVulkanFunctions;

typedef struct VmaAllocatorCreateInfo {
    uint32_t flags;
    VkPhysicalDevice physicalDevice;
    VkDevice device;
    VkDeviceSize preferredLargeHeapBlockSize;
    const VkAllocationCallbacks *pAllocationCallbacks;
    const VmaDeviceMemoryCallbacks *pDeviceMemoryCallbacks;
    const VkDeviceSize *pHeapSizeLimit;
    const VmaVulkanFunctions *pVulkanFunctions;
    VkInstance instance;
    uint32_t vulkanApiVersion;
    const uint32_t *pTypeExternalMemoryHandleTypes;
} VmaAllocatorCreateInfo;

typedef struct VmaAllocatorInfo {
    VkInstance instance;
    VkPhysicalDevice physicalDevice;
    VkDevice device;
} VmaAllocatorInfo;

typedef struct VmaStatistics {
    uint32_t blockCount, allocationCount;
    VkDeviceSize blockBytes, allocationBytes;
} VmaStatistics;

typedef struct VmaDetailedStatistics {
    VmaStatistics statistics;
    uint32_t unusedRangeCount;
    VkDeviceSize allocationSizeMin, allocationSizeMax, unusedRangeSizeMin, unusedRangeSizeMax;
} VmaDetailedStatistics;

typedef struct VmaTotalStatistics {
    VmaDetailedStatistics memoryType[VK_MAX_MEMORY_TYPES];
    VmaDetailedStatistics memoryHeap[VK_MAX_MEMORY_HEAPS];
    VmaDetailedStatistics total;
} VmaTotalStatistics;

typedef struct VmaBudget {
    VmaStatistics statistics;
    VkDeviceSize usage, budget;
} VmaBudget;

typedef struct VmaAllocationCreateInfo {
    uint32_t flags;
    VmaMemoryUsage usage;
    VkFlags requiredFlags, preferredFlags;
    uint32_t memoryTypeBits;
    VmaPool pool;
    void *pUserData;
    float priority;
    VkDeviceSize minAlignment;
} VmaAllocationCreateInfo;

typedef struct VmaPoolCreateInfo {
    uint32_t memoryTypeIndex, flags;
    VkDeviceSize blockSize;
    size_t minBlockCount, maxBlockCount;
    float priority;
    VkDeviceSize minAllocationAlignment;
    void *pMemoryAllocateNext;
} VmaPoolCreateInfo;

typedef struct VmaAllocationInfo {
    uint32_t memoryType;
    VkDeviceMemory deviceMemory;
    VkDeviceSize offset, size;
    void *pMappedData;
    void *pUserData;
    const char *pName;
} VmaAllocationInfo;

typedef struct VmaAllocationInfo2 {
    VmaAllocationInfo allocationInfo;
    VkDeviceSize blockSize;
    VkBool32 dedicatedMemory;
} VmaAllocationInfo2;

typedef VkBool32 (*PFN_vmaCheckDefragmentationBreakFunction)(void *);

typedef struct VmaDefragmentationInfo {
    uint32_t flags;
    VmaPool pool;
    VkDeviceSize maxBytesPerPass;
    uint32_t maxAllocationsPerPass;
    PFN_vmaCheckDefragmentationBreakFunction pfnBreakCallback;
    void *pBreakCallbackUserData;
} VmaDefragmentationInfo;

typedef struct VmaDefragmentationMove {
    int32_t operation;
    VmaAllocation srcAllocation, dstTmpAllocation;
} VmaDefragmentationMove;

typedef struct VmaDefragmentationPassMoveInfo {
    uint32_t moveCount;
    VmaDefragmentationMove *pMoves;
} VmaDefragmentationPassMoveInfo;

typedef struct VmaDefragmentationStats {
    VkDeviceSize bytesMoved, bytesFreed;
    uint32_t allocationsMoved, deviceMemoryBlocksFreed;
} VmaDefragmentationStats;

typedef struct VmaVirtualBlockCreateInfo {
    VkDeviceSize size;
    uint32_t flags;
    const VkAllocationCallbacks *pAllocationCallbacks;
} VmaVirtualBlockCreateInfo;

typedef struct VmaVirtualAllocationCreateInfo {
    VkDeviceSize size, alignment;
    uint32_t flags;
    void *pUserData;
} VmaVirtualAllocationCreateInfo;

typedef struct VmaVirtualAllocationInfo {
    VkDeviceSize offset, size;
    void *pUserData;
} VmaVirtualAllocationInfo;

VMA_CALL_PRE VkResult VMA_CALL_POST vmaImportVulkanFunctionsFromVolk(
    const VmaAllocatorCreateInfo *pAllocatorCreateInfo, VmaVulkanFunctions *pDstVulkanFunctions);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateAllocator(
    const VmaAllocatorCreateInfo *pCreateInfo, VmaAllocator *pAllocator);
VMA_CALL_PRE void VMA_CALL_POST vmaDestroyAllocator(VmaAllocator allocator);
VMA_CALL_PRE void VMA_CALL_POST vmaGetAllocatorInfo(VmaAllocator allocator, VmaAllocatorInfo *pAllocatorInfo);
VMA_CALL_PRE void VMA_CALL_POST vmaGetPhysicalDeviceProperties(
    VmaAllocator allocator, const VkPhysicalDeviceProperties **ppPhysicalDeviceProperties);
VMA_CALL_PRE void VMA_CALL_POST vmaGetMemoryProperties(
    VmaAllocator allocator, const VkPhysicalDeviceMemoryProperties **ppPhysicalDeviceMemoryProperties);
VMA_CALL_PRE void VMA_CALL_POST vmaGetMemoryTypeProperties(
    VmaAllocator allocator, uint32_t memoryTypeIndex, VkMemoryPropertyFlags *pFlags);
VMA_CALL_PRE void VMA_CALL_POST vmaSetCurrentFrameIndex(VmaAllocator allocator, uint32_t frameIndex);
VMA_CALL_PRE void VMA_CALL_POST vmaCalculateStatistics(VmaAllocator allocator, VmaTotalStatistics *pStats);
VMA_CALL_PRE void VMA_CALL_POST vmaGetHeapBudgets(VmaAllocator allocator, VmaBudget *pBudgets);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaFindMemoryTypeIndex(
    VmaAllocator allocator, uint32_t memoryTypeBits, const VmaAllocationCreateInfo *pAllocationCreateInfo,
    uint32_t *pMemoryTypeIndex);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaFindMemoryTypeIndexForBufferInfo(
    VmaAllocator allocator, const VkBufferCreateInfo *pBufferCreateInfo,
    const VmaAllocationCreateInfo *pAllocationCreateInfo, uint32_t *pMemoryTypeIndex);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaFindMemoryTypeIndexForImageInfo(
    VmaAllocator allocator, const VkImageCreateInfo *pImageCreateInfo,
    const VmaAllocationCreateInfo *pAllocationCreateInfo, uint32_t *pMemoryTypeIndex);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreatePool(
    VmaAllocator allocator, const VmaPoolCreateInfo *pCreateInfo, VmaPool *pPool);
VMA_CALL_PRE void VMA_CALL_POST vmaDestroyPool(VmaAllocator allocator, VmaPool pool);
VMA_CALL_PRE void VMA_CALL_POST vmaGetPoolStatistics(
    VmaAllocator allocator, VmaPool pool, VmaStatistics *pPoolStats);
VMA_CALL_PRE void VMA_CALL_POST vmaCalculatePoolStatistics(
    VmaAllocator allocator, VmaPool pool, VmaDetailedStatistics *pPoolStats);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCheckPoolCorruption(VmaAllocator allocator, VmaPool pool);
VMA_CALL_PRE void VMA_CALL_POST vmaGetPoolName(VmaAllocator allocator, VmaPool pool, const char **ppName);
VMA_CALL_PRE void VMA_CALL_POST vmaSetPoolName(VmaAllocator allocator, VmaPool pool, const char *pName);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaAllocateMemory(
    VmaAllocator allocator, const VkMemoryRequirements *pVkMemoryRequirements,
    const VmaAllocationCreateInfo *pCreateInfo, VmaAllocation *pAllocation, VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaAllocateDedicatedMemory(
    VmaAllocator allocator, const VkMemoryRequirements *pVkMemoryRequirements,
    const VmaAllocationCreateInfo *pCreateInfo, void *pMemoryAllocateNext, VmaAllocation *pAllocation,
    VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaAllocateMemoryPages(
    VmaAllocator allocator, const VkMemoryRequirements *pVkMemoryRequirements,
    const VmaAllocationCreateInfo *pCreateInfo, size_t allocationCount, VmaAllocation *pAllocations,
    VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaAllocateMemoryForBuffer(
    VmaAllocator allocator, VkBuffer buffer, const VmaAllocationCreateInfo *pCreateInfo,
    VmaAllocation *pAllocation, VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaAllocateMemoryForImage(
    VmaAllocator allocator, VkImage image, const VmaAllocationCreateInfo *pCreateInfo,
    VmaAllocation *pAllocation, VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE void VMA_CALL_POST vmaFreeMemory(VmaAllocator allocator, VmaAllocation allocation);
VMA_CALL_PRE void VMA_CALL_POST vmaFreeMemoryPages(
    VmaAllocator allocator, size_t allocationCount, const VmaAllocation *pAllocations);
VMA_CALL_PRE void VMA_CALL_POST vmaGetAllocationInfo(
    VmaAllocator allocator, VmaAllocation allocation, VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE void VMA_CALL_POST vmaGetAllocationInfo2(
    VmaAllocator allocator, VmaAllocation allocation, VmaAllocationInfo2 *pAllocationInfo);
VMA_CALL_PRE void VMA_CALL_POST vmaSetAllocationUserData(
    VmaAllocator allocator, VmaAllocation allocation, void *pUserData);
VMA_CALL_PRE void VMA_CALL_POST vmaSetAllocationName(
    VmaAllocator allocator, VmaAllocation allocation, const char *pName);
VMA_CALL_PRE void VMA_CALL_POST vmaGetAllocationMemoryProperties(
    VmaAllocator allocator, VmaAllocation allocation, VkMemoryPropertyFlags *pFlags);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaGetMemoryWin32Handle(
    VmaAllocator allocator, VmaAllocation allocation, void *hTargetProcess, void **pHandle);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaGetMemoryWin32Handle2(
    VmaAllocator allocator, VmaAllocation allocation, uint32_t handleType, void *hTargetProcess, void **pHandle);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaMapMemory(
    VmaAllocator allocator, VmaAllocation allocation, void **ppData);
VMA_CALL_PRE void VMA_CALL_POST vmaUnmapMemory(VmaAllocator allocator, VmaAllocation allocation);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaFlushAllocation(
    VmaAllocator allocator, VmaAllocation allocation, VkDeviceSize offset, VkDeviceSize size);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaInvalidateAllocation(
    VmaAllocator allocator, VmaAllocation allocation, VkDeviceSize offset, VkDeviceSize size);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaFlushAllocations(
    VmaAllocator allocator, uint32_t allocationCount, const VmaAllocation *allocations,
    const VkDeviceSize *offsets, const VkDeviceSize *sizes);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaInvalidateAllocations(
    VmaAllocator allocator, uint32_t allocationCount, const VmaAllocation *allocations,
    const VkDeviceSize *offsets, const VkDeviceSize *sizes);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCopyMemoryToAllocation(
    VmaAllocator allocator, const void *pSrcHostPointer, VmaAllocation dstAllocation,
    VkDeviceSize dstAllocationLocalOffset, VkDeviceSize size);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCopyAllocationToMemory(
    VmaAllocator allocator, VmaAllocation srcAllocation, VkDeviceSize srcAllocationLocalOffset,
    void *pDstHostPointer, VkDeviceSize size);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCheckCorruption(VmaAllocator allocator, uint32_t memoryTypeBits);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaBeginDefragmentation(
    VmaAllocator allocator, const VmaDefragmentationInfo *pInfo, VmaDefragmentationContext *pContext);
VMA_CALL_PRE void VMA_CALL_POST vmaEndDefragmentation(
    VmaAllocator allocator, VmaDefragmentationContext context, VmaDefragmentationStats *pStats);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaBeginDefragmentationPass(
    VmaAllocator allocator, VmaDefragmentationContext context, VmaDefragmentationPassMoveInfo *pPassInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaEndDefragmentationPass(
    VmaAllocator allocator, VmaDefragmentationContext context, VmaDefragmentationPassMoveInfo *pPassInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaBindBufferMemory(
    VmaAllocator allocator, VmaAllocation allocation, VkBuffer buffer);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaBindBufferMemory2(
    VmaAllocator allocator, VmaAllocation allocation, VkDeviceSize allocationLocalOffset, VkBuffer buffer,
    const void *pNext);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaBindImageMemory(
    VmaAllocator allocator, VmaAllocation allocation, VkImage image);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaBindImageMemory2(
    VmaAllocator allocator, VmaAllocation allocation, VkDeviceSize allocationLocalOffset, VkImage image,
    const void *pNext);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateBuffer(
    VmaAllocator allocator, const VkBufferCreateInfo *pBufferCreateInfo,
    const VmaAllocationCreateInfo *pAllocationCreateInfo, VkBuffer *pBuffer, VmaAllocation *pAllocation,
    VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateBufferWithAlignment(
    VmaAllocator allocator, const VkBufferCreateInfo *pBufferCreateInfo,
    const VmaAllocationCreateInfo *pAllocationCreateInfo, VkDeviceSize minAlignment, VkBuffer *pBuffer,
    VmaAllocation *pAllocation, VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateDedicatedBuffer(
    VmaAllocator allocator, const VkBufferCreateInfo *pBufferCreateInfo,
    const VmaAllocationCreateInfo *pAllocationCreateInfo, void *pMemoryAllocateNext, VkBuffer *pBuffer,
    VmaAllocation *pAllocation, VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateAliasingBuffer(
    VmaAllocator allocator, VmaAllocation allocation, const VkBufferCreateInfo *pBufferCreateInfo,
    VkBuffer *pBuffer);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateAliasingBuffer2(
    VmaAllocator allocator, VmaAllocation allocation, VkDeviceSize allocationLocalOffset,
    const VkBufferCreateInfo *pBufferCreateInfo, VkBuffer *pBuffer);
VMA_CALL_PRE void VMA_CALL_POST vmaDestroyBuffer(
    VmaAllocator allocator, VkBuffer buffer, VmaAllocation allocation);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateImage(
    VmaAllocator allocator, const VkImageCreateInfo *pImageCreateInfo,
    const VmaAllocationCreateInfo *pAllocationCreateInfo, VkImage *pImage, VmaAllocation *pAllocation,
    VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateDedicatedImage(
    VmaAllocator allocator, const VkImageCreateInfo *pImageCreateInfo,
    const VmaAllocationCreateInfo *pAllocationCreateInfo, void *pMemoryAllocateNext, VkImage *pImage,
    VmaAllocation *pAllocation, VmaAllocationInfo *pAllocationInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateAliasingImage(
    VmaAllocator allocator, VmaAllocation allocation, const VkImageCreateInfo *pImageCreateInfo,
    VkImage *pImage);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateAliasingImage2(
    VmaAllocator allocator, VmaAllocation allocation, VkDeviceSize allocationLocalOffset,
    const VkImageCreateInfo *pImageCreateInfo, VkImage *pImage);
VMA_CALL_PRE void VMA_CALL_POST vmaDestroyImage(
    VmaAllocator allocator, VkImage image, VmaAllocation allocation);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaCreateVirtualBlock(
    const VmaVirtualBlockCreateInfo *pCreateInfo, VmaVirtualBlock *pVirtualBlock);
VMA_CALL_PRE void VMA_CALL_POST vmaDestroyVirtualBlock(VmaVirtualBlock virtualBlock);
VMA_CALL_PRE VkBool32 VMA_CALL_POST vmaIsVirtualBlockEmpty(VmaVirtualBlock virtualBlock);
VMA_CALL_PRE void VMA_CALL_POST vmaGetVirtualAllocationInfo(
    VmaVirtualBlock virtualBlock, VmaVirtualAllocation allocation, VmaVirtualAllocationInfo *pVirtualAllocInfo);
VMA_CALL_PRE VkResult VMA_CALL_POST vmaVirtualAllocate(
    VmaVirtualBlock virtualBlock, const VmaVirtualAllocationCreateInfo *pCreateInfo,
    VmaVirtualAllocation *pAllocation, VkDeviceSize *pOffset);
VMA_CALL_PRE void VMA_CALL_POST vmaVirtualFree(VmaVirtualBlock virtualBlock, VmaVirtualAllocation allocation);
VMA_CALL_PRE void VMA_CALL_POST vmaClearVirtualBlock(VmaVirtualBlock virtualBlock);
VMA_CALL_PRE void VMA_CALL_POST vmaSetVirtualAllocationUserData(
    VmaVirtualBlock virtualBlock, VmaVirtualAllocation allocation, void *pUserData);
VMA_CALL_PRE void VMA_CALL_POST vmaGetVirtualBlockStatistics(
    VmaVirtualBlock virtualBlock, VmaStatistics *pStats);
VMA_CALL_PRE void VMA_CALL_POST vmaCalculateVirtualBlockStatistics(
    VmaVirtualBlock virtualBlock, VmaDetailedStatistics *pStats);
VMA_CALL_PRE void VMA_CALL_POST vmaBuildVirtualBlockStatsString(
    VmaVirtualBlock virtualBlock, char **ppStatsString, VkBool32 detailedMap);
VMA_CALL_PRE void VMA_CALL_POST vmaFreeVirtualBlockStatsString(
    VmaVirtualBlock virtualBlock, char *pStatsString);
VMA_CALL_PRE void VMA_CALL_POST vmaBuildStatsString(
    VmaAllocator allocator, char **ppStatsString, VkBool32 detailedMap);
VMA_CALL_PRE void VMA_CALL_POST vmaFreeStatsString(VmaAllocator allocator, char *pStatsString);

#ifdef __cplusplus
}
#endif

#endif /* AMD_VULKAN_MEMORY_ALLOCATOR_H */
