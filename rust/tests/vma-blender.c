/* GPU-free Blender-style VMA device smoke against the Rust cdylib.
 *
 * Mirrors Blender GHOST (`GHOST_DeviceVK::init_memory_allocator`) and OpenXR
 * staging: Vulkan 1.2, BUFFER_DEVICE_ADDRESS, EXT_MEMORY_PRIORITY,
 * KHR_MAINTENANCE4, EXT_MEMORY_BUDGET; vertex/index/uniform buffers; GPU-only
 * images; mapped sequential-write staging; custom pool; 3.4 dedicated +
 * minAlignment. Fake Vulkan lives in this file - no GPU, no libvulkan. */
#include "vk_mem_alloc.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef VK_API_VERSION_1_2
#define VK_API_VERSION_1_2 VK_MAKE_VERSION(1, 2, 0)
#endif
#ifndef VK_BUFFER_USAGE_TRANSFER_SRC_BIT
#define VK_BUFFER_USAGE_TRANSFER_SRC_BIT 0x00000001u
#define VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT 0x00000010u
#define VK_BUFFER_USAGE_INDEX_BUFFER_BIT 0x00000040u
#define VK_BUFFER_USAGE_VERTEX_BUFFER_BIT 0x00000080u
#define VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT 0x00020000u
#define VK_IMAGE_USAGE_TRANSFER_DST_BIT 0x00000002u
#define VK_IMAGE_USAGE_SAMPLED_BIT 0x00000004u
#define VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT 0x00000001u
#define VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT 0x00000002u
#define VK_MEMORY_PROPERTY_HOST_COHERENT_BIT 0x00000004u
#endif

static void die(const char *msg) {
    fprintf(stderr, "%s\n", msg);
    exit(1);
}

typedef struct FakeMemAlloc {
    int32_t sType;
    const void *pNext;
    uint64_t allocationSize;
    uint32_t memoryTypeIndex;
} FakeMemAlloc;

static uint64_t g_next = 1;
static uint64_t g_buf_size[4096];
static uint64_t g_img_size[4096];
static uint8_t *g_mem[4096];
static uint64_t g_mem_id[4096];
static int g_nmem;

static void fake_props(VkPhysicalDevice d, VkPhysicalDeviceProperties *p) {
    (void)d;
    memset(p, 0, sizeof(*p));
    p->limits.nonCoherentAtomSize = 256;
    p->limits.bufferImageGranularity = 1;
}
static void fake_mem(VkPhysicalDevice d, VkPhysicalDeviceMemoryProperties *p) {
    (void)d;
    memset(p, 0, sizeof(*p));
    p->memoryHeapCount = 1;
    p->memoryHeaps[0].size = 64u * 1024u * 1024u;
    p->memoryTypeCount = 2;
    p->memoryTypes[0].propertyFlags = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
    p->memoryTypes[1].propertyFlags =
        VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
}
static VkResult fake_alloc(VkDevice d, const FakeMemAlloc *info, const void *cb, uint64_t *out) {
    (void)d;
    (void)cb;
    if (g_nmem >= 4096) {
        return VK_ERROR_OUT_OF_DEVICE_MEMORY;
    }
    uint64_t id = g_next++;
    size_t n = (size_t)info->allocationSize;
    if (n == 0) {
        n = 1;
    }
    g_mem[g_nmem] = calloc(n, 1);
    g_mem_id[g_nmem] = id;
    g_nmem++;
    *out = id;
    return VK_SUCCESS;
}
static void fake_free(VkDevice d, uint64_t m, const void *cb) {
    (void)d;
    (void)m;
    (void)cb;
}
static VkResult fake_map(VkDevice d, uint64_t mem, uint64_t off, uint64_t sz, uint32_t flags, void **pp) {
    (void)d;
    (void)sz;
    (void)flags;
    for (int i = 0; i < g_nmem; i++) {
        if (g_mem_id[i] == mem) {
            *pp = g_mem[i] + (size_t)off;
            return VK_SUCCESS;
        }
    }
    return VK_ERROR_UNKNOWN;
}
static void fake_unmap(VkDevice d, uint64_t m) {
    (void)d;
    (void)m;
}
static VkResult fake_flush(VkDevice d, uint32_t n, const void *r) {
    (void)d;
    (void)n;
    (void)r;
    return VK_SUCCESS;
}
static VkResult fake_bind(VkDevice d, uint64_t obj, uint64_t m, uint64_t o) {
    (void)d;
    (void)obj;
    (void)m;
    (void)o;
    return VK_SUCCESS;
}
static void fake_buf_req(VkDevice d, uint64_t b, VkMemoryRequirements *r) {
    (void)d;
    uint64_t sz = (b < 4096 && g_buf_size[b]) ? g_buf_size[b] : 4096;
    r->size = sz < 256 ? 256 : sz;
    r->alignment = 256;
    r->memoryTypeBits = 3;
}
static void fake_img_req(VkDevice d, uint64_t i, VkMemoryRequirements *r) {
    (void)d;
    uint64_t sz = (i < 4096 && g_img_size[i]) ? g_img_size[i] : 8192;
    r->size = sz < 256 ? 256 : sz;
    r->alignment = 256;
    r->memoryTypeBits = 1;
}
static VkResult fake_create_buf(VkDevice d, const VkBufferCreateInfo *ci, const void *cb, uint64_t *out) {
    (void)d;
    (void)cb;
    *out = g_next++;
    if (*out < 4096) {
        g_buf_size[*out] = ci && ci->size ? ci->size : 4096;
    }
    return VK_SUCCESS;
}
static void fake_destroy_buf(VkDevice d, uint64_t b, const void *cb) {
    (void)d;
    (void)b;
    (void)cb;
}
static VkResult fake_create_img(VkDevice d, const VkImageCreateInfo *ci, const void *cb, uint64_t *out) {
    (void)d;
    (void)cb;
    *out = g_next++;
    uint64_t sz = 8192;
    if (ci) {
        sz = (uint64_t)ci->extent.width * ci->extent.height * (ci->extent.depth ? ci->extent.depth : 1) * 4;
        if (sz < 256) {
            sz = 256;
        }
    }
    if (*out < 4096) {
        g_img_size[*out] = sz;
    }
    return VK_SUCCESS;
}
static void fake_destroy_img(VkDevice d, uint64_t i, const void *cb) {
    (void)d;
    (void)i;
    (void)cb;
}
static PFN_vkVoidFunction fake_gipa(VkInstance i, const char *n) {
    (void)i;
    (void)n;
    return NULL;
}
static PFN_vkVoidFunction fake_gdpa(VkDevice d, const char *n) {
    (void)d;
    (void)n;
    return NULL;
}

static VmaAllocator make_blender_allocator(void) {
    VmaVulkanFunctions fns;
    memset(&fns, 0, sizeof(fns));
    fns.vkGetInstanceProcAddr = fake_gipa;
    fns.vkGetDeviceProcAddr = fake_gdpa;
    fns.vkGetPhysicalDeviceProperties = (void *)fake_props;
    fns.vkGetPhysicalDeviceMemoryProperties = (void *)fake_mem;
    fns.vkAllocateMemory = (void *)fake_alloc;
    fns.vkFreeMemory = (void *)fake_free;
    fns.vkMapMemory = (void *)fake_map;
    fns.vkUnmapMemory = (void *)fake_unmap;
    fns.vkFlushMappedMemoryRanges = (void *)fake_flush;
    fns.vkInvalidateMappedMemoryRanges = (void *)fake_flush;
    fns.vkBindBufferMemory = (void *)fake_bind;
    fns.vkBindImageMemory = (void *)fake_bind;
    fns.vkGetBufferMemoryRequirements = (void *)fake_buf_req;
    fns.vkGetImageMemoryRequirements = (void *)fake_img_req;
    fns.vkCreateBuffer = (void *)fake_create_buf;
    fns.vkDestroyBuffer = (void *)fake_destroy_buf;
    fns.vkCreateImage = (void *)fake_create_img;
    fns.vkDestroyImage = (void *)fake_destroy_img;

    VmaAllocatorCreateInfo ci;
    memset(&ci, 0, sizeof(ci));
    ci.physicalDevice = (VkPhysicalDevice)(uintptr_t)1;
    ci.device = (VkDevice)(uintptr_t)2;
    ci.instance = (VkInstance)(uintptr_t)3;
    ci.pVulkanFunctions = &fns;
    ci.vulkanApiVersion = VK_API_VERSION_1_2;
    ci.flags = VMA_ALLOCATOR_CREATE_BUFFER_DEVICE_ADDRESS_BIT |
               VMA_ALLOCATOR_CREATE_EXT_MEMORY_PRIORITY_BIT |
               VMA_ALLOCATOR_CREATE_KHR_MAINTENANCE4_BIT |
               VMA_ALLOCATOR_CREATE_EXT_MEMORY_BUDGET_BIT;
    VmaAllocator a = NULL;
    if (vmaCreateAllocator(&ci, &a) != VK_SUCCESS || !a) {
        die("vmaCreateAllocator");
    }
    return a;
}

static void make_buf(VmaAllocator a, VkDeviceSize size, uint32_t usage, const VmaAllocationCreateInfo *aci,
                    VkBuffer *buf, VmaAllocation *alloc) {
    VkBufferCreateInfo bci;
    memset(&bci, 0, sizeof(bci));
    bci.size = size;
    bci.usage = usage;
    VmaAllocationInfo info;
    memset(&info, 0, sizeof(info));
    if (vmaCreateBuffer(a, &bci, aci, buf, alloc, &info) != VK_SUCCESS) {
        die("vmaCreateBuffer");
    }
}

int main(void) {
    if (VMA_VERSION != VK_MAKE_VERSION(3, 4, 0)) {
        die("VMA_VERSION is not 3.4.0");
    }
    VmaAllocator a = make_blender_allocator();

    VmaAllocationCreateInfo gpu;
    memset(&gpu, 0, sizeof(gpu));
    gpu.usage = VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE;
    VmaAllocationCreateInfo mapped;
    memset(&mapped, 0, sizeof(mapped));
    mapped.usage = VMA_MEMORY_USAGE_AUTO;
    mapped.flags = VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT | VMA_ALLOCATION_CREATE_MAPPED_BIT;
    VmaAllocationCreateInfo img_ci;
    memset(&img_ci, 0, sizeof(img_ci));
    img_ci.usage = VMA_MEMORY_USAGE_GPU_ONLY;

    VkBuffer verts[48];
    VmaAllocation vert_a[48];
    for (int i = 0; i < 48; i++) {
        make_buf(a, (VkDeviceSize)(1024 + i * 128),
                 VK_BUFFER_USAGE_VERTEX_BUFFER_BIT | VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT, &gpu,
                 &verts[i], &vert_a[i]);
    }
    /* Hundreds of extra buffer alloc/free (mesh-edit churn). */
    {
        VkBuffer churn[256];
        VmaAllocation churn_a[256];
        for (int i = 0; i < 256; i++) {
            make_buf(a, (VkDeviceSize)(64 + i * 8), VK_BUFFER_USAGE_VERTEX_BUFFER_BIT, &gpu,
                     &churn[i], &churn_a[i]);
        }
        for (int i = 0; i < 256; i += 2) {
            vmaDestroyBuffer(a, churn[i], churn_a[i]);
            churn[i] = 0;
            churn_a[i] = NULL;
        }
        for (int i = 0; i < 256; i += 2) {
            make_buf(a, 128, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT, &gpu, &churn[i], &churn_a[i]);
        }
        for (int i = 0; i < 256; i++) {
            vmaDestroyBuffer(a, churn[i], churn_a[i]);
        }
    }
    VkBuffer indices[48];
    VmaAllocation idx_a[48];
    for (int i = 0; i < 48; i++) {
        make_buf(a, (VkDeviceSize)(512 + i * 64), VK_BUFFER_USAGE_INDEX_BUFFER_BIT, &gpu, &indices[i],
                 &idx_a[i]);
    }
    VkBuffer uniforms[16];
    VmaAllocation uni_a[16];
    for (int i = 0; i < 16; i++) {
        VkBufferCreateInfo bci;
        memset(&bci, 0, sizeof(bci));
        bci.size = 256;
        bci.usage = VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT;
        VmaAllocationInfo info;
        memset(&info, 0, sizeof(info));
        if (vmaCreateBuffer(a, &bci, &mapped, &uniforms[i], &uni_a[i], &info) != VK_SUCCESS ||
            !info.pMappedData) {
            die("uniform mapped");
        }
        memset(info.pMappedData, 0xAB, 16);
    }

    VkImage images[8];
    VmaAllocation img_a[8];
    for (int i = 0; i < 8; i++) {
        VkImageCreateInfo ici;
        memset(&ici, 0, sizeof(ici));
        ici.extent.width = (uint32_t)(32 + i * 16);
        ici.extent.height = ici.extent.width;
        ici.extent.depth = 1;
        ici.mipLevels = 1;
        ici.arrayLayers = 1;
        ici.samples = 1;
        ici.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
        if (vmaCreateImage(a, &ici, &img_ci, &images[i], &img_a[i], NULL) != VK_SUCCESS) {
            die("vmaCreateImage");
        }
    }

    /* Mesh-edit style: free every other vertex buffer, then allocate more. */
    for (int i = 0; i < 48; i += 2) {
        vmaDestroyBuffer(a, verts[i], vert_a[i]);
        verts[i] = 0;
        vert_a[i] = NULL;
    }
    VkBuffer extra[12];
    VmaAllocation extra_a[12];
    for (int i = 0; i < 12; i++) {
        make_buf(a, (VkDeviceSize)(2048 + i * 32), VK_BUFFER_USAGE_VERTEX_BUFFER_BIT, &gpu, &extra[i],
                 &extra_a[i]);
    }

    VmaTotalStatistics st;
    memset(&st, 0, sizeof(st));
    vmaCalculateStatistics(a, &st);
    if (st.total.statistics.allocationCount < 48 || st.total.statistics.allocationBytes == 0) {
        die("statistics");
    }
    VmaBudget budgets[VK_MAX_MEMORY_HEAPS];
    memset(budgets, 0, sizeof(budgets));
    vmaGetHeapBudgets(a, budgets);
    if (budgets[0].budget == 0) {
        die("budget");
    }

    VmaDefragmentationInfo di;
    memset(&di, 0, sizeof(di));
    VmaDefragmentationContext ctx = NULL;
    if (vmaBeginDefragmentation(a, &di, &ctx) != VK_SUCCESS) {
        die("defrag");
    }
    vmaEndDefragmentation(a, ctx, NULL);

    /* 3.4 minAlignment + dedicated buffer/image. */
    VmaAllocationCreateInfo aligned = gpu;
    aligned.minAlignment = 4096;
    VkBuffer abuf = 0;
    VmaAllocation aalloc = NULL;
    VmaAllocationInfo ainfo;
    memset(&ainfo, 0, sizeof(ainfo));
    VkBufferCreateInfo bci;
    memset(&bci, 0, sizeof(bci));
    bci.size = 64;
    bci.usage = VK_BUFFER_USAGE_VERTEX_BUFFER_BIT;
    if (vmaCreateBuffer(a, &bci, &aligned, &abuf, &aalloc, &ainfo) != VK_SUCCESS) {
        die("minAlignment");
    }
    if (ainfo.offset % 4096 != 0) {
        die("minAlignment offset");
    }
    VkBuffer dbuf = 0;
    VmaAllocation dalloc = NULL;
    if (vmaCreateDedicatedBuffer(a, &bci, &gpu, NULL, &dbuf, &dalloc, NULL) != VK_SUCCESS) {
        die("vmaCreateDedicatedBuffer");
    }
    VkImageCreateInfo dici;
    memset(&dici, 0, sizeof(dici));
    dici.extent.width = 64;
    dici.extent.height = 64;
    dici.extent.depth = 1;
    dici.mipLevels = 1;
    dici.arrayLayers = 1;
    dici.samples = 1;
    dici.usage = VK_IMAGE_USAGE_SAMPLED_BIT;
    VkImage dimg = 0;
    VmaAllocation dimg_a = NULL;
    if (vmaCreateDedicatedImage(a, &dici, &img_ci, NULL, &dimg, &dimg_a, NULL) != VK_SUCCESS) {
        die("vmaCreateDedicatedImage");
    }

    /* XR-style mapped staging. */
    VkBuffer stage = 0;
    VmaAllocation stage_a = NULL;
    VmaAllocationInfo sinfo;
    memset(&sinfo, 0, sizeof(sinfo));
    memset(&bci, 0, sizeof(bci));
    bci.size = 65536;
    bci.usage = VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
    if (vmaCreateBuffer(a, &bci, &mapped, &stage, &stage_a, &sinfo) != VK_SUCCESS || !sinfo.pMappedData) {
        die("staging");
    }
    memset(sinfo.pMappedData, 1, 32);
    void *p = NULL;
    if (vmaMapMemory(a, stage_a, &p) != VK_SUCCESS || !p) {
        die("vmaMapMemory");
    }
    vmaUnmapMemory(a, stage_a);
    vmaDestroyBuffer(a, stage, stage_a);

    vmaDestroyBuffer(a, abuf, aalloc);
    vmaDestroyBuffer(a, dbuf, dalloc);
    vmaDestroyImage(a, dimg, dimg_a);
    for (int i = 0; i < 48; i++) {
        if (verts[i]) {
            vmaDestroyBuffer(a, verts[i], vert_a[i]);
        }
        vmaDestroyBuffer(a, indices[i], idx_a[i]);
    }
    for (int i = 0; i < 12; i++) {
        vmaDestroyBuffer(a, extra[i], extra_a[i]);
    }
    for (int i = 0; i < 16; i++) {
        vmaDestroyBuffer(a, uniforms[i], uni_a[i]);
    }
    for (int i = 0; i < 8; i++) {
        vmaDestroyImage(a, images[i], img_a[i]);
    }
    vmaDestroyAllocator(a);
    for (int i = 0; i < g_nmem; i++) {
        free(g_mem[i]);
    }
    puts("ok");
    return 0;
}
