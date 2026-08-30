/* GPU-free virtual allocator smoke against the Rust VMA cdylib. */
#include "vk_mem_alloc.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void die(const char *msg) {
    fprintf(stderr, "%s\n", msg);
    exit(1);
}

int main(void) {
    VmaVirtualBlockCreateInfo bci;
    memset(&bci, 0, sizeof(bci));
    bci.size = 1024 * 1024;
    VmaVirtualBlock block = NULL;
    if (vmaCreateVirtualBlock(&bci, &block) != VK_SUCCESS || !block) {
        die("vmaCreateVirtualBlock");
    }
    if (!vmaIsVirtualBlockEmpty(block)) {
        die("expected empty");
    }

    VmaVirtualAllocationCreateInfo aci;
    memset(&aci, 0, sizeof(aci));
    aci.size = 4096;
    aci.alignment = 256;
    aci.flags = VMA_ALLOCATION_CREATE_STRATEGY_MIN_TIME_BIT;
    VmaVirtualAllocation a1 = 0, a2 = 0;
    VkDeviceSize o1 = 0, o2 = 0;
    if (vmaVirtualAllocate(block, &aci, &a1, &o1) != VK_SUCCESS) {
        die("alloc1");
    }
    if (vmaVirtualAllocate(block, &aci, &a2, &o2) != VK_SUCCESS) {
        die("alloc2");
    }
    if (o1 % 256 != 0 || o2 % 256 != 0) {
        die("alignment");
    }
    if (o1 == o2) {
        die("overlap");
    }
    if (vmaIsVirtualBlockEmpty(block)) {
        die("not empty");
    }

    VmaVirtualAllocationInfo vi;
    memset(&vi, 0, sizeof(vi));
    vmaGetVirtualAllocationInfo(block, a1, &vi);
    if (vi.size != 4096 || vi.offset != o1) {
        die("get info");
    }
    vmaSetVirtualAllocationUserData(block, a1, (void *)(uintptr_t)0xabc);
    vmaGetVirtualAllocationInfo(block, a1, &vi);
    if (vi.pUserData != (void *)(uintptr_t)0xabc) {
        die("user data");
    }

    VmaStatistics st;
    memset(&st, 0, sizeof(st));
    vmaGetVirtualBlockStatistics(block, &st);
    if (st.allocationCount != 2 || st.allocationBytes != 8192 || st.blockBytes != 1024 * 1024) {
        die("stats");
    }

    char *json = NULL;
    vmaBuildVirtualBlockStatsString(block, &json, VK_FALSE);
    if (!json || !strstr(json, "allocationCount")) {
        die("stats string");
    }
    vmaFreeVirtualBlockStatsString(block, json);

    vmaVirtualFree(block, a1);
    vmaVirtualFree(block, a2);
    if (!vmaIsVirtualBlockEmpty(block)) {
        die("empty after free");
    }

    /* Reuse after free. */
    if (vmaVirtualAllocate(block, &aci, &a1, &o1) != VK_SUCCESS) {
        die("realloc");
    }
    vmaClearVirtualBlock(block);
    if (!vmaIsVirtualBlockEmpty(block)) {
        die("clear");
    }

    /* Linear bump. */
    VmaVirtualBlockCreateInfo linear = bci;
    linear.flags = VMA_VIRTUAL_BLOCK_CREATE_LINEAR_ALGORITHM_BIT;
    VmaVirtualBlock lb = NULL;
    if (vmaCreateVirtualBlock(&linear, &lb) != VK_SUCCESS) {
        die("linear block");
    }
    VkDeviceSize lo1 = 0, lo2 = 0;
    VmaVirtualAllocation la1 = 0, la2 = 0;
    if (vmaVirtualAllocate(lb, &aci, &la1, &lo1) != VK_SUCCESS) {
        die("linear 1");
    }
    if (vmaVirtualAllocate(lb, &aci, &la2, &lo2) != VK_SUCCESS) {
        die("linear 2");
    }
    if (lo2 <= lo1) {
        die("linear bump");
    }
    vmaDestroyVirtualBlock(lb);
    vmaDestroyVirtualBlock(block);

    /* Exhaustion returns OUT_OF_DEVICE_MEMORY and UINT64_MAX offset. */
    VmaVirtualBlockCreateInfo tiny = bci;
    tiny.size = 100;
    VmaVirtualBlock tb = NULL;
    if (vmaCreateVirtualBlock(&tiny, &tb) != VK_SUCCESS) {
        die("tiny");
    }
    aci.size = 1000;
    VkDeviceSize off = 0;
    VmaVirtualAllocation fail = 1;
    if (vmaVirtualAllocate(tb, &aci, &fail, &off) != VK_ERROR_OUT_OF_DEVICE_MEMORY) {
        die("exhaust result");
    }
    if (fail != 0 || off != ~(VkDeviceSize)0) {
        die("exhaust handles");
    }
    vmaDestroyVirtualBlock(tb);

    puts("ok");
    return 0;
}
