// SPDX-License-Identifier: MIT
// Pinned SDK primitives only. Scan/join/commit/retry/UI policy stays in Rust.
#include <cstdint>
#include <cstring>
#include <esp_event.h>
#include <esp_heap_caps.h>
#include <esp_heap_caps_init.h>
#include <heap_memory_layout.h>
#include <esp_log.h>
#include <esp_netif.h>
#include <esp_private/phy.h>
#include <esp_random.h>
#include <esp_wifi.h>
#include <freertos/FreeRTOS.h>
#include <freertos/timers.h>
#include <mbedtls/gcm.h>
#include <mbedtls/sha256.h>
#include <mbedtls/platform_util.h>
#include <atomic>
#include "../../salamander/nvs_guard/nvs_guard.h"

// Same pinned no-timer-task link boundary as frozen update_wifi.h. A real
// timers.c user fails the link instead of adding a task to reading boots.
extern "C" BaseType_t xTimerPendFunctionCallFromISR(PendedFunction_t, void*, uint32_t, BaseType_t*) {
  return pdFAIL;
}
extern "C" void newt_wifi_fence(std::uint8_t raised) {
  newt::firmware::fenceFlashForWifi(raised != 0);
}
extern "C" std::int32_t newt_wifi_lend_memory(std::uintptr_t start, std::uintptr_t end, std::int32_t* bytes) {
  constexpr std::uint32_t caps[SOC_MEMORY_TYPE_NO_PRIOS] = {
      0, MALLOC_CAP_8BIT | MALLOC_CAP_32BIT | MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL | MALLOC_CAP_DEFAULT, 0};
  const auto error = heap_caps_add_region_with_caps(caps, start, end);
  *bytes = static_cast<std::int32_t>(end - start);
  return error;
}
extern "C" std::uint8_t newt_wifi_fenced() {
  return newt::firmware::flashFencedForWifi();
}
extern "C" void newt_wifi_refusals(std::uint8_t* out) {
  const auto result = newt::firmware::flashFenceRefusals();
  out[0] = result.opens; out[1] = result.writes;
}
extern "C" void newt_wifi_silence_log() { esp_log_level_set("wifi", ESP_LOG_NONE); }
extern "C" std::int32_t newt_wifi_create_loop() { return esp_event_loop_create_default(); }
extern "C" std::int32_t newt_wifi_delete_loop() { return esp_event_loop_delete_default(); }
extern "C" std::int32_t newt_wifi_init_no_nvs() {
  wifi_init_config_t init = WIFI_INIT_CONFIG_DEFAULT();
  init.nvs_enable = 0;
  return esp_wifi_init(&init);
}
extern "C" void newt_wifi_usb_bbpll() { phy_bbpll_en_usb(true); }
extern "C" std::int32_t newt_wifi_storage_ram() { return esp_wifi_set_storage(WIFI_STORAGE_RAM); }
extern "C" std::int32_t newt_wifi_mode_station() { return esp_wifi_set_mode(WIFI_MODE_STA); }
extern "C" std::int32_t newt_wifi_start() { return esp_wifi_start(); }
extern "C" std::int32_t newt_wifi_scan() { return esp_wifi_scan_start(nullptr, true); }
extern "C" std::int32_t newt_wifi_scan_count(std::uint16_t* count) { return esp_wifi_scan_get_ap_num(count); }
extern "C" std::int32_t newt_wifi_scan_record(std::uint8_t* name, std::int32_t* rssi, std::uint8_t* open) {
  wifi_ap_record_t record;
  const auto result = esp_wifi_scan_get_ap_record(&record);
  if (result == ESP_OK) {
    static_assert(sizeof(record.ssid) == 33);
    std::memcpy(name, record.ssid, sizeof(record.ssid));
    *rssi = record.rssi; *open = record.authmode == WIFI_AUTH_OPEN;
  }
  return result;
}
extern "C" std::int32_t newt_wifi_clear_ap_list() { return esp_wifi_clear_ap_list(); }
extern "C" void newt_wifi_radio_draw(std::uint8_t* fresh) { esp_fill_random(fresh, 32); }
extern "C" std::uint32_t newt_wifi_random_word() { return esp_random(); }
extern "C" std::int32_t newt_wifi_stop() { return esp_wifi_stop(); }
extern "C" std::int32_t newt_wifi_deinit() { return esp_wifi_deinit(); }
extern "C" std::int32_t newt_wifi_clear_config() {
  wifi_config_t empty{};
  return esp_wifi_set_config(WIFI_IF_STA, &empty);
}
namespace {
struct JoinEvents {
  std::atomic<bool> connected{false}, addressed{false};
  std::atomic<std::uint16_t> reason{0};
};
struct JoinState {
  JoinEvents events;
  wifi_config_t config{};
  esp_netif_t* station = nullptr;
};
void note_join(void* context, esp_event_base_t base, std::int32_t id, void* data) {
  auto& seen = static_cast<JoinState*>(context)->events;
  if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) seen.addressed = true;
  if (base != WIFI_EVENT) return;
  if (id == WIFI_EVENT_STA_CONNECTED) seen.connected = true;
  if (id == WIFI_EVENT_STA_DISCONNECTED) {
    const auto reason = static_cast<const wifi_event_sta_disconnected_t*>(data)->reason;
    seen.reason = reason ? reason : 1;
  }
}
}
extern "C" void newt_wifi_with_join(void* context, void (*callback)(void*, void*)) {
  JoinState scope;
  callback(context, &scope);
}
extern "C" std::int32_t newt_wifi_netif_init() { return esp_netif_init(); }
extern "C" std::uint8_t newt_wifi_join_station(void* scope) {
  auto& state = *static_cast<JoinState*>(scope);
  state.station = esp_netif_create_default_wifi_sta();
  return state.station != nullptr;
}
extern "C" std::int32_t newt_wifi_join_register_wifi(void* scope) {
  return esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, note_join, scope);
}
extern "C" std::int32_t newt_wifi_join_register_ip(void* scope) {
  return esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, note_join, scope);
}
extern "C" void newt_wifi_join_prepare(void* scope, const std::uint8_t* name,
    const std::uint8_t* password, std::size_t bytes) {
  auto& config = static_cast<JoinState*>(scope)->config;
  config = {};
  std::memcpy(config.sta.ssid, name, std::strlen(reinterpret_cast<const char*>(name)));
  std::memcpy(config.sta.password, password, bytes);
}
extern "C" std::int32_t newt_wifi_join_set_config(void* scope) {
  return esp_wifi_set_config(WIFI_IF_STA, &static_cast<JoinState*>(scope)->config);
}
extern "C" void newt_wifi_join_wipe_config(void* scope) {
  mbedtls_platform_zeroize(&static_cast<JoinState*>(scope)->config, sizeof(wifi_config_t));
}
extern "C" std::int32_t newt_wifi_connect() { return esp_wifi_connect(); }
extern "C" std::uint8_t newt_wifi_join_addressed(void* scope) {
  return static_cast<JoinState*>(scope)->events.addressed.load();
}
extern "C" std::uint16_t newt_wifi_join_reason(void* scope) {
  return static_cast<JoinState*>(scope)->events.reason.load();
}
extern "C" std::uint8_t newt_wifi_join_connected(void* scope) {
  return static_cast<JoinState*>(scope)->events.connected.load();
}
extern "C" std::int32_t newt_wifi_disconnect() { return esp_wifi_disconnect(); }
extern "C" void newt_wifi_join_destroy_station(void* scope) {
  auto& station = static_cast<JoinState*>(scope)->station;
  if (station) esp_netif_destroy_default_wifi(station);
  station = nullptr;
}
extern "C" std::int32_t newt_wifi_join_unregister_ip() {
  return esp_event_handler_unregister(IP_EVENT, IP_EVENT_STA_GOT_IP, note_join);
}
extern "C" std::int32_t newt_wifi_join_unregister_wifi() {
  return esp_event_handler_unregister(WIFI_EVENT, ESP_EVENT_ANY_ID, note_join);
}
extern "C" std::int32_t newt_wifi_sha256(const std::uint8_t* input, std::size_t bytes, std::uint8_t* output) {
  return mbedtls_sha256(input, bytes, output, 0);
}
extern "C" std::int32_t newt_wifi_gcm_seal(const std::uint8_t* key, const std::uint8_t* nonce,
    const std::uint8_t* header, const std::uint8_t* plain, std::uint8_t* cipher, std::uint8_t* tag) {
  mbedtls_gcm_context context;
  mbedtls_gcm_init(&context);
  auto result = mbedtls_gcm_setkey(&context, MBEDTLS_CIPHER_ID_AES, key, 256);
  if (result == 0) result = mbedtls_gcm_crypt_and_tag(&context, MBEDTLS_GCM_ENCRYPT, 98,
      nonce, 12, header, 12, plain, cipher, 16, tag);
  mbedtls_gcm_free(&context);
  return result;
}
extern "C" std::int32_t newt_wifi_gcm_open(const std::uint8_t* key, const std::uint8_t* nonce,
    const std::uint8_t* header, const std::uint8_t* cipher, const std::uint8_t* tag, std::uint8_t* plain) {
  mbedtls_gcm_context context;
  mbedtls_gcm_init(&context);
  auto result = mbedtls_gcm_setkey(&context, MBEDTLS_CIPHER_ID_AES, key, 256);
  if (result == 0) result = mbedtls_gcm_auth_decrypt(&context, 98, nonce, 12, header, 12, tag, 16, cipher, plain);
  mbedtls_gcm_free(&context);
  return result;
}
