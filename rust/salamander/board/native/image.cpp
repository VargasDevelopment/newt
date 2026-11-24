// SPDX-License-Identifier: MIT
#include "board.h"
#include <esp_app_desc.h>
#include <esp_ota_ops.h>
#include <esp_partition.h>
#include <mbedtls/sha256.h>
extern "C" {
const char* nb_running_image_version() {
    return esp_app_get_description()->version;
}
int32_t nb_running_image_state(uint32_t* out) {
    if (!out) return NB_NULL;
    const auto* running = esp_ota_get_running_partition();
    if (!running) return ESP_ERR_NOT_FOUND;
    esp_ota_img_states_t state;
    const auto result = esp_ota_get_state_partition(running, &state);
    if (result == ESP_OK) *out = static_cast<uint32_t>(state);
    return result;
}
int32_t nb_mark_running_image_valid() {
    return esp_ota_mark_app_valid_cancel_rollback();
}
const void* nb_image_app_partition(uint32_t slot) {
    if (slot > 1) return nullptr;
    return esp_partition_find_first(ESP_PARTITION_TYPE_APP,
        slot ? ESP_PARTITION_SUBTYPE_APP_OTA_1 : ESP_PARTITION_SUBTYPE_APP_OTA_0, nullptr);
}
const void* nb_image_ota_partition() {
    return esp_partition_find_first(ESP_PARTITION_TYPE_DATA, ESP_PARTITION_SUBTYPE_DATA_OTA, nullptr);
}
const void* nb_image_running_partition() { return esp_ota_get_running_partition(); }
int32_t nb_image_partition_bytes(const void* partition, uint32_t* out) {
    if (!partition || !out) return ESP_ERR_INVALID_ARG;
    *out = static_cast<const esp_partition_t*>(partition)->size;
    return ESP_OK;
}
int32_t nb_image_partition_read(const void* partition, uint32_t at, uint8_t* out, size_t bytes) {
    if (!partition) return ESP_ERR_INVALID_ARG;
    return esp_partition_read(static_cast<const esp_partition_t*>(partition), at, out, bytes);
}
int32_t nb_image_partition_erase(const void* partition, uint32_t at, uint32_t bytes) {
    if (!partition) return ESP_ERR_INVALID_ARG;
    return esp_partition_erase_range(static_cast<const esp_partition_t*>(partition), at, bytes);
}
int32_t nb_image_partition_write(const void* partition, uint32_t at, const uint8_t* input, size_t bytes) {
    if (!partition) return ESP_ERR_INVALID_ARG;
    return esp_partition_write(static_cast<const esp_partition_t*>(partition), at, input, bytes);
}
typedef uint32_t (*nb_image_hash_callback)(void*, void*);
uint32_t nb_image_hash_scope(nb_image_hash_callback callback, void* call) {
    if (!callback || !call) return 0;
    mbedtls_sha256_context context;
    mbedtls_sha256_init(&context);
    const auto result = callback(&context, call);
    mbedtls_sha256_free(&context);
    return result;
}
int32_t nb_image_hash_start(void* context) {
    return mbedtls_sha256_starts(static_cast<mbedtls_sha256_context*>(context), 0);
}
int32_t nb_image_hash_update(void* context, const uint8_t* input, size_t bytes) {
    return mbedtls_sha256_update(static_cast<mbedtls_sha256_context*>(context), input, bytes);
}
int32_t nb_image_hash_finish(void* context, uint8_t* out) {
    return mbedtls_sha256_finish(static_cast<mbedtls_sha256_context*>(context), out);
}
}
