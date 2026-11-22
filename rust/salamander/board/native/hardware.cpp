// SPDX-License-Identifier: MIT
#include "hardware.h"

#include <BatteryMonitor.h>
#include <BoardConfig.h>
#include <Esp.h>
#include <esp_mac.h>
#include <esp_random.h>
#include <freertos/FreeRTOS.h>
#include <freertos/task.h>

static_assert(FREEINK_DEVICE_X3 && !FREEINK_DEVICE_X4);
static_assert(sizeof(BatteryMonitor) == 12 && alignof(BatteryMonitor) == 4,
              "same pinned SDK owner as frozen main.cpp");
static_assert(sizeof(StackType_t) == 1 && sizeof(UBaseType_t) == sizeof(uint32_t),
              "pinned ESP-IDF stack watermark is already bytes");
static_assert(sizeof(esp_err_t) == sizeof(int32_t));
#if defined(CONFIG_SOC_IEEE802154_SUPPORTED) && CONFIG_SOC_IEEE802154_SUPPORTED
#error "X3 factory identity uses the pinned C3 six-byte MAC"
#endif
namespace {
// The one boot-lifetime SDK instance replaces main.cpp's owner in Y only.
// Its SDK-owned Wire initialization/state is not copied or wrapped here.
BatteryMonitor battery;
}

int32_t nb_hw_read_battery(struct nb_hw_battery *out) {
    if (!out) return NB_HW_NULL;
    const auto status = battery.readStatus();
    *out = {status.percentage, status.millivolts,
            uint8_t(status.percentageKnown), uint8_t(status.millivoltsKnown),
            uint8_t(status.chargingKnown), uint8_t(status.charging)};
    return NB_HW_OK;
}

int32_t nb_hw_free_heap(uint32_t *out) {
    if (!out) return NB_HW_NULL;
    *out = ESP.getFreeHeap();
    return NB_HW_OK;
}

int32_t nb_hw_min_free_heap(uint32_t *out) {
    if (!out) return NB_HW_NULL;
    *out = ESP.getMinFreeHeap();
    return NB_HW_OK;
}

int32_t nb_hw_largest_heap_block(uint32_t *out) {
    if (!out) return NB_HW_NULL;
    *out = ESP.getMaxAllocHeap();
    return NB_HW_OK;
}

int32_t nb_hw_current_task_stack_low(uint32_t *out) {
    if (!out) return NB_HW_NULL;
#if INCLUDE_uxTaskGetStackHighWaterMark == 1
    *out = uxTaskGetStackHighWaterMark(nullptr);
    return NB_HW_OK;
#else
    return NB_HW_UNAVAILABLE;
#endif
}

int32_t nb_hw_factory_mac(struct nb_hw_factory_mac *out) {
    if (!out) return NB_HW_NULL;
    // Initialize primitive bytes even if the SDK fails without writing them.
    // Rust checks sdk_status before returning any identity bytes to its caller.
    *out = {};
    out->sdk_status = esp_efuse_mac_get_default(out->bytes);
    return NB_HW_OK;
}

int32_t nb_hw_random_word(uint32_t *out) {
    if (!out) return NB_HW_NULL;
    *out = esp_random();
    return NB_HW_OK;
}
