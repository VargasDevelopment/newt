/* SPDX-License-Identifier: MIT
 * Host fake of the image partition SDK leaves (board/native/image.cpp). The
 * firmware's own install, rollback and firmware-switch policy runs over
 * NEWT_PREVIEW_DIR/partitions/{app0,app1,otadata}.bin. Bytes past a file's
 * end read as erased flash (0xff), so a missing file is an empty slot.
 *   slot.running  0 (default) | 1: the slot this image runs from
 *   image.state   the SDK OTA state of the running image (default 2, valid),
 *                 or "unreadable"; partitions/state, once written, wins
 *   slot.fail     PART-OP names ("otadata-read", "app1-erase"; OP is read,
 *                 erase or write, or reread: a read after this boot wrote
 *                 that part) whose SDK call fails after doing nothing
 *   slot.lose     PART names whose writes report success but never land
 * freeze point: slot.write (the Installing page, at the first app write). */
#include "fixture.h"
#include "../../../firmware/native/firmware_version.h"
#include "../../../tests/reader_providers/fake/mbedtls/sha256.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct partition { const char *name; uint32_t bytes; };
/* The X3 table: two 0x770000-byte app slots and one two-sector otadata. */
static const struct partition partitions[] = {{"app0", 0x770000}, {"app1", 0x770000}, {"otadata", 0x2000}};
static int written[3];
enum { ESP_FAIL = -1, ESP_ERR_INVALID_ARG = 0x102 };

static const char *file_of(const struct partition *part) {
    char relative[64];
    snprintf(relative, sizeof relative, "partitions/%s.bin", part->name);
    return preview_path(relative);
}
static int faulted(const char *key, const struct partition *part, const char *op) {
    const char *list = preview_conf(key);
    if (!list) return 0;
    char name[32];
    snprintf(name, sizeof name, op ? "%s-%s" : "%s", part->name, op ? op : "");
    return strstr(list, name) != NULL;
}
static int valid(const struct partition *part, uint32_t at, size_t bytes) {
    return part && at <= part->bytes && bytes <= part->bytes - at;
}
/* Write `bytes` at `at`, creating the file and leaving any gap erased. */
static int store(const struct partition *part, uint32_t at, const uint8_t *input, uint8_t fill, size_t bytes) {
    FILE *file = fopen(file_of(part), "r+b");
    if (!file) file = fopen(file_of(part), "w+b");
    if (!file) return ESP_FAIL;
    fseek(file, 0, SEEK_END);
    for (long end = ftell(file); end < (long)at; end++) fputc(0xff, file);
    fseek(file, (long)at, SEEK_SET);
    for (size_t i = 0; i < bytes; i++) fputc(input ? input[i] : fill, file);
    return fclose(file) == 0 ? 0 : ESP_FAIL;
}

const char *nb_running_image_version(void) {
    static const char version[32] = NEWT_FIRMWARE_VERSION;
    return version;
}
static int running_slot(void) { return preview_conf_long("slot.running", 0) == 1; }
int32_t nb_running_image_state(uint32_t *out) {
    FILE *file = fopen(preview_path("partitions/state"), "rb");
    long state = preview_conf_long("image.state", 2);
    if (file) { if (fscanf(file, "%ld", &state) != 1) state = 2; fclose(file); }
    else if (preview_conf_is("image.state", "unreadable")) return ESP_FAIL;
    *out = (uint32_t)state;
    return 0;
}
int32_t nb_mark_running_image_valid(void) {
    FILE *file = fopen(preview_path("partitions/state"), "wb");
    if (!file) return ESP_FAIL;
    fputs("2\n", file);
    return fclose(file) == 0 ? 0 : ESP_FAIL;
}
const void *nb_image_app_partition(uint32_t slot) { return slot < 2 ? &partitions[slot] : NULL; }
const void *nb_image_ota_partition(void) { return &partitions[2]; }
const void *nb_image_running_partition(void) { return &partitions[running_slot()]; }
int32_t nb_image_partition_bytes(const void *partition, uint32_t *out) {
    if (!partition || !out) return ESP_ERR_INVALID_ARG;
    *out = ((const struct partition *)partition)->bytes;
    return 0;
}
int32_t nb_image_partition_read(const void *partition, uint32_t at, uint8_t *out, size_t bytes) {
    const struct partition *part = partition;
    if (!valid(part, at, bytes)) return ESP_ERR_INVALID_ARG;
    if (faulted("slot.fail", part, "read")) return ESP_FAIL;
    if (written[part - partitions] && faulted("slot.fail", part, "reread")) return ESP_FAIL;
    memset(out, 0xff, bytes);
    FILE *file = fopen(file_of(part), "rb");
    if (!file) return 0;
    if (fseek(file, (long)at, SEEK_SET) == 0) (void)fread(out, 1, bytes, file);
    fclose(file);
    return 0;
}
int32_t nb_image_partition_erase(const void *partition, uint32_t at, uint32_t bytes) {
    const struct partition *part = partition;
    if (!valid(part, at, bytes) || at % 4096 || bytes % 4096) return ESP_ERR_INVALID_ARG;
    if (faulted("slot.fail", part, "erase")) return ESP_FAIL;
    return store(part, at, NULL, 0xff, bytes);
}
int32_t nb_image_partition_write(const void *partition, uint32_t at, const uint8_t *input, size_t bytes) {
    const struct partition *part = partition;
    if (!valid(part, at, bytes)) return ESP_ERR_INVALID_ARG;
    if (part != &partitions[2]) preview_freeze_at("slot.write");
    if (faulted("slot.fail", part, "write")) return ESP_FAIL;
    if (faulted("slot.lose", part, NULL)) return 0;
    written[part - partitions] = 1;
    return store(part, at, input, 0, bytes);
}

/* The download and slot checks hash with the host mbedTLS fake (a real
 * SHA-256), so a fixture image's appended digest must be its true one. */
typedef uint32_t (*hash_callback)(void *, void *);
uint32_t nb_image_hash_scope(hash_callback callback, void *call) {
    if (!callback || !call) return 0;
    mbedtls_sha256_context context;
    mbedtls_sha256_init(&context);
    uint32_t result = callback(&context, call);
    mbedtls_sha256_free(&context);
    return result;
}
int32_t nb_image_hash_start(void *context) { return mbedtls_sha256_starts(context, 0); }
int32_t nb_image_hash_update(void *context, const uint8_t *input, size_t bytes) {
    return mbedtls_sha256_update(context, input, bytes);
}
int32_t nb_image_hash_finish(void *context, uint8_t *out) { return mbedtls_sha256_finish(context, out); }
