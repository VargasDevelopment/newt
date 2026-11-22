// SPDX-License-Identifier: MIT

// Fixed ESP32-C3 single-core boundary; each Rust loan ends before native SDK.
#include "integration.h"
#include "nvs_guard.h"
#include <esp_partition.h>
#include <nvs.h>
#include <nvs_flash.h>
#include <esp_idf_version.h>
#include <sdkconfig.h>
#include <freertos/FreeRTOS.h>
#include <freertos/portmacro.h>
#if ESP_IDF_VERSION != ESP_IDF_VERSION_VAL(5, 5, 2)
#error "Reaudit guard ABI for changed IDF"
#endif
#if !defined(CONFIG_IDF_TARGET_ESP32C3) || CONFIG_IDF_TARGET_ESP32C3 != 1 || \
    !defined(CONFIG_FREERTOS_UNICORE) || CONFIG_FREERTOS_UNICORE != 1 || \
    !defined(CONFIG_FREERTOS_NUMBER_OF_CORES) || CONFIG_FREERTOS_NUMBER_OF_CORES != 1
#error "Guard ownership requires the pinned ESP32-C3 unicore port"
#endif
static_assert(configNUM_CORES == 1);
static_assert(ESP_OK == 0 && ESP_ERR_INVALID_STATE == 0x103);
static_assert(ESP_ERR_NVS_NO_FREE_PAGES == 0x110d && ESP_ERR_NVS_NEW_VERSION_FOUND == 0x1110);
static_assert(NVS_READONLY == 0 && ESP_PARTITION_TYPE_DATA == 1 && ESP_PARTITION_SUBTYPE_DATA_NVS == 2);
static_assert(sizeof(esp_err_t) == sizeof(std::int32_t));
static_assert(sizeof(NewtGuardRefusals) == 2 && alignof(NewtGuardRefusals) == 1);
namespace {
// This port ignores the mux argument and supports nesting in task/ISR context.
// Scope only one leaf: no native SDK call, allocation or blocking while held.
class GuardCritical final {
 public:
  GuardCritical() { portENTER_CRITICAL(nullptr); }
  ~GuardCritical() { portEXIT_CRITICAL(nullptr); }
  GuardCritical(const GuardCritical&) = delete;
  GuardCritical& operator=(const GuardCritical&) = delete;
};
template<class Function, class... Args>
auto guardCall(Function function, Args... args) {
  const GuardCritical critical;
  return function(newt_guard_owner(), args...);
}
// Only SDK metadata projection. All accept/refuse decisions are Rust policy.
std::uint8_t present(const esp_partition_t* p) { return p != nullptr; }
std::uint8_t kind(const esp_partition_t* p) { return p ? p->type : 0; }
std::uint8_t subtype(const esp_partition_t* p) { return p ? p->subtype : 0; }
std::size_t extent(const esp_partition_t* p) { return p ? p->size : 0; }
}
extern "C" {
esp_err_t __real_nvs_flash_init();
esp_err_t __real_esp_partition_erase_range(const esp_partition_t*, std::size_t, std::size_t);
esp_err_t __real_esp_partition_write(const esp_partition_t*, std::size_t, const void*, std::size_t);
esp_err_t __real_esp_partition_write_raw(const esp_partition_t*, std::size_t, const void*, std::size_t);
esp_err_t __real_nvs_open(const char*, nvs_open_mode_t, nvs_handle_t*);
esp_err_t __wrap_nvs_flash_init() {
  if (!guardCall(newt_guard_begin_init)) return ESP_ERR_INVALID_STATE;
  // begin export has returned: no Rust loan spans this reentrant native call.
  const auto result = __real_nvs_flash_init();
  return guardCall(newt_guard_finish_init, result);
}
esp_err_t __wrap_esp_partition_erase_range(const esp_partition_t* p, std::size_t offset, std::size_t size) {
  if (guardCall(newt_guard_deny_erase, present(p), kind(p), subtype(p), extent(p), offset, size))
    return ESP_ERR_INVALID_STATE;
  return __real_esp_partition_erase_range(p, offset, size);
}
esp_err_t __wrap_esp_partition_write(const esp_partition_t* p, std::size_t offset, const void* data, std::size_t size) {
  if (guardCall(newt_guard_deny_write, present(p), kind(p), subtype(p), extent(p)))
    return ESP_ERR_INVALID_STATE;
  return __real_esp_partition_write(p, offset, data, size);
}
esp_err_t __wrap_esp_partition_write_raw(const esp_partition_t* p, std::size_t offset, const void* data, std::size_t size) {
  if (guardCall(newt_guard_deny_write, present(p), kind(p), subtype(p), extent(p)))
    return ESP_ERR_INVALID_STATE;
  return __real_esp_partition_write_raw(p, offset, data, size);
}
esp_err_t __wrap_nvs_open(const char* name, nvs_open_mode_t mode, nvs_handle_t* handle) {
  if (guardCall(newt_guard_deny_open, static_cast<std::int32_t>(mode))) return ESP_ERR_INVALID_STATE;
  return __real_nvs_open(name, mode, handle);
}
esp_err_t __wrap_nvs_flash_erase() { return guardCall(newt_guard_deny_format); }
esp_err_t __wrap_nvs_flash_erase_partition(const char*) { return guardCall(newt_guard_deny_format); }
esp_err_t __wrap_nvs_flash_erase_partition_ptr(const esp_partition_t*) { return guardCall(newt_guard_deny_format); }
}
namespace newt::firmware {
bool nvsStorageReady() { return guardCall(newt_guard_ready) != 0; }
NvsIssue nvsStorageIssue() { return static_cast<NvsIssue>(guardCall(newt_guard_issue)); }
const char* nvsStorageProblem() { return guardCall(newt_guard_problem); }
void fenceFlashForWifi(bool raised) { guardCall(newt_guard_fence, raised); }
bool flashFencedForWifi() { return guardCall(newt_guard_fenced) != 0; }
FlashFenceRefusals flashFenceRefusals() {
  const auto r = guardCall(newt_guard_refusals);
  return {r.opens, r.writes};
}
}
