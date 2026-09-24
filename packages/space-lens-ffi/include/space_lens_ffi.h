#ifndef SPACE_LENS_FFI_H
#define SPACE_LENS_FFI_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SPACE_LENS_FFI_ABI_VERSION 1u

typedef struct SlAbiInfo {
    uint32_t struct_size;
    uint32_t abi_version;
} SlAbiInfo;

typedef struct SlProgressEvent {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t phase;
    uint64_t processed;
    uint64_t total;
    uint64_t logical_bytes;
    uint64_t allocated_bytes;
    void *user_data;
} SlProgressEvent;

typedef struct SlOwnedBuffer {
    uint8_t *ptr;
    uintptr_t len;
} SlOwnedBuffer;

typedef struct SlICloudPlan SlICloudPlan;

typedef struct SlICloudProgress {
    uint32_t struct_size;
    uint32_t abi_version;
    uint64_t processed;
    uint64_t total;
    uint64_t evicted;
    uint64_t failed;
    uint64_t freed_bytes;
    uint64_t active;
    uint8_t running;
    uint8_t paused;
    uint8_t cancelled;
    uint8_t reserved;
} SlICloudProgress;

uint32_t sl_abi_version(void);
SlAbiInfo sl_abi_info(void);
uint32_t sl_capability_count(void);
SlOwnedBuffer sl_scan_directory_json(const char *path);
SlOwnedBuffer sl_icloud_plan_json(const char *path);
SlICloudPlan *sl_icloud_plan_create(const char *path);
SlOwnedBuffer sl_icloud_plan_handle_json(const SlICloudPlan *plan);
SlOwnedBuffer sl_icloud_plan_execute(const SlICloudPlan *plan, uint32_t max_concurrency);
SlICloudProgress sl_icloud_progress(const SlICloudPlan *plan);
void sl_icloud_pause(const SlICloudPlan *plan);
void sl_icloud_resume(const SlICloudPlan *plan);
void sl_icloud_cancel(const SlICloudPlan *plan);
void sl_icloud_plan_free(SlICloudPlan *plan);
void sl_free_buffer(SlOwnedBuffer buffer);

#ifdef __cplusplus
}
#endif

#endif
