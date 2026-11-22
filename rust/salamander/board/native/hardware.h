/* SPDX-License-Identifier: MIT */
#ifndef NEWT_C3_HARDWARE_H
#define NEWT_C3_HARDWARE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* Private ABI for the Rust app, excluding main.cpp's BatteryMonitor owner.
 * Exactly one SDK BatteryMonitor lives from boot to reset.
 * Sole acquired X3 UI SDK owner, fixed ACTIVE profile and caller-owned
 * HardwareReader permission, used sequentially between Wire/display/SD loans;
 * no native/task/ISR/reentrant sensor access or foreign
 * unwind. Outputs are private, live, aligned and do not overlap SDK objects.
 * OK means the SDK call returned, not that all battery fields are known/fresh.
 * Null/unavailable calls leave output untouched. No clock or app policy. */
enum { NB_HW_OK = 0, NB_HW_NULL = 1, NB_HW_UNAVAILABLE = 2 };

struct nb_hw_battery {
    uint16_t percentage;
    uint16_t millivolts;
    uint8_t percentage_known;
    uint8_t millivolts_known;
    uint8_t charging_known;
    uint8_t charging;
};

/* SDK status is separate from bridge availability. Failed SDK reads never
 * publish the bytes as an identity. No fingerprint encoding/hashing here. */
struct nb_hw_factory_mac {
    int32_t sdk_status;
    uint8_t bytes[6];
};

int32_t nb_hw_read_battery(struct nb_hw_battery *out);
int32_t nb_hw_free_heap(uint32_t *out);
int32_t nb_hw_min_free_heap(uint32_t *out);
int32_t nb_hw_largest_heap_block(uint32_t *out);
/* Pinned ESP-IDF returns BYTES for the calling task, not vanilla FreeRTOS
 * stack words. No multiplication, task switch or extra measurement. */
int32_t nb_hw_current_task_stack_low(uint32_t *out);
int32_t nb_hw_factory_mac(struct nb_hw_factory_mac *out);
/* Exactly esp_random's raw word, including zero. No entropy/freshness claim. */
int32_t nb_hw_random_word(uint32_t *out);
#ifdef __cplusplus
}
static_assert(sizeof(nb_hw_battery) == 8 && alignof(nb_hw_battery) == 2);
static_assert(offsetof(nb_hw_battery, percentage) == 0 &&
              offsetof(nb_hw_battery, millivolts) == 2 &&
              offsetof(nb_hw_battery, percentage_known) == 4 &&
              offsetof(nb_hw_battery, millivolts_known) == 5 &&
              offsetof(nb_hw_battery, charging_known) == 6 &&
              offsetof(nb_hw_battery, charging) == 7);
static_assert(sizeof(struct nb_hw_factory_mac) == 12 && alignof(struct nb_hw_factory_mac) == 4);
static_assert(offsetof(struct nb_hw_factory_mac, sdk_status) == 0 &&
              offsetof(struct nb_hw_factory_mac, bytes) == 4);
#endif
#endif
