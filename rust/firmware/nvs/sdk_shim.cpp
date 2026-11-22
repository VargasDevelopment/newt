// SPDX-License-Identifier: MIT

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <esp_idf_version.h>
#include <nvs.h>
#include "../../salamander/nvs_guard/nvs_guard.h"

#if ESP_IDF_VERSION != ESP_IDF_VERSION_VAL(5, 5, 2)
#error "This shim requires a fresh contract audit for a different IDF"
#endif
static_assert(sizeof(nvs_handle_t) == sizeof(std::uint32_t));
static_assert(sizeof(esp_err_t) == sizeof(std::int32_t));
static_assert(ESP_ERR_NVS_NOT_FOUND == 0x1102);
static_assert(ESP_ERR_NVS_INVALID_LENGTH == 0x110c);

// Ordinary SDK symbols intentionally retain the existing --wrap protections.
// No __real_* calls, initialization, fence changes, or durability policy here.
extern "C" {
std::uint8_t newt_nvs_ready() { return newt::firmware::nvsStorageReady(); }
std::uint8_t newt_nvs_issue() { return static_cast<std::uint8_t>(newt::firmware::nvsStorageIssue()); }
std::int32_t newt_nvs_open(const char* name, std::uint8_t readonly, std::uint32_t* handle) {
  return nvs_open(name, readonly ? NVS_READONLY : NVS_READWRITE, handle);
}
void newt_nvs_close(std::uint32_t handle) { nvs_close(handle); }
std::int32_t newt_nvs_get(std::uint32_t handle, const char* key, void* output, std::size_t* size) {
  return nvs_get_blob(handle, key, output, size);
}
std::int32_t newt_nvs_set(std::uint32_t handle, const char* key, const void* bytes, std::size_t size) {
  return nvs_set_blob(handle, key, bytes, size);
}
std::int32_t newt_nvs_commit(std::uint32_t handle) { return nvs_commit(handle); }
std::int32_t newt_nvs_erase(std::uint32_t handle, const char* key) { return nvs_erase_key(handle, key); }
std::int32_t newt_nvs_visit(std::uint8_t (*visit)(void*, const char*, const char*, std::uint8_t), void* context) {
  nvs_iterator_t iterator = nullptr;
  auto result = nvs_entry_find("nvs", nullptr, NVS_TYPE_ANY, &iterator);
  while (result == ESP_OK) {
    nvs_entry_info_t entry{};
    result = nvs_entry_info(iterator, &entry);
    if (result != ESP_OK) break;
    if (!std::memchr(entry.namespace_name, 0, sizeof(entry.namespace_name)) ||
        !std::memchr(entry.key, 0, sizeof(entry.key)) ||
        !visit(context, entry.namespace_name, entry.key, entry.type == NVS_TYPE_BLOB)) {
      result = ESP_FAIL;
      break;
    }
    result = nvs_entry_next(&iterator);
  }
  nvs_release_iterator(iterator);
  return result == ESP_ERR_NVS_NOT_FOUND ? ESP_OK : result;
}
}
