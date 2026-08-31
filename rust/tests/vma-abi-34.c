/* VMA 3.4 ABI layout: version macro, new struct fields, dedicated symbols. */
#include "vk_mem_alloc.h"
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static void die(const char *msg) {
    fprintf(stderr, "%s\n", msg);
    exit(1);
}

int main(void) {
    if (VMA_VERSION != VK_MAKE_VERSION(3, 4, 0)) {
        die("VMA_VERSION is not 3.4.0");
    }
    if (offsetof(VmaAllocationCreateInfo, minAlignment) < offsetof(VmaAllocationCreateInfo, priority)) {
        die("minAlignment must follow priority");
    }
    if (offsetof(VmaVulkanFunctions, vkGetPhysicalDeviceProperties2KHR) <
        offsetof(VmaVulkanFunctions, vkGetMemoryWin32HandleKHR)) {
        die("vkGetPhysicalDeviceProperties2KHR must follow vkGetMemoryWin32HandleKHR");
    }
    /* Taking addresses fails the link if the DSO dropped a 3.4 export. */
    volatile uintptr_t sink = 0;
    sink |= (uintptr_t)vmaAllocateDedicatedMemory;
    sink |= (uintptr_t)vmaCreateDedicatedBuffer;
    sink |= (uintptr_t)vmaCreateDedicatedImage;
    sink |= (uintptr_t)vmaGetMemoryWin32Handle2;
    sink |= (uintptr_t)vmaCreateBufferWithAlignment;
    (void)sink;
    puts("ok");
    return 0;
}
