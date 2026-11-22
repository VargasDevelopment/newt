/* SPDX-License-Identifier: MIT
 * Host fake of the update boot's Wi-Fi SDK leaves (firmware/native/wifi.cpp).
 * The firmware's own update_radio scan/join policy runs over these; each
 * step reports what fixture.conf says the radio heard:
 *   wifi.networks  rssi,open,name entries joined by ';' (scan order)
 *   wifi.first     how many the first scan hears; each rescan hears one more
 *   wifi.scan      failed | none: the first scan's outcome (later ones hear)
 *   wifi.join      connected (default) | wrong-password | not-found |
 *                  no-address | timeout | failed
 * freeze points: wifi.scan (the Scanning page), wifi.join (Joining). */
#include "fixture.h"
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static uint8_t fenced, logs_silenced;
static unsigned scans;
static struct { char name[33]; int32_t rssi; uint8_t open; } heard[16];
static unsigned heard_count, heard_next;

void newt_wifi_fence(uint8_t raised) { fenced = raised != 0; }
uint8_t newt_wifi_fenced(void) { return fenced; }
/* The NVS guard's two refusal counters; the host store never refuses. */
void newt_wifi_refusals(uint8_t *out) { out[0] = 0; out[1] = 0; }
void newt_wifi_silence_log(void) { logs_silenced = 1; }
int32_t newt_wifi_create_loop(void) { return 0; }
int32_t newt_wifi_delete_loop(void) { return 0; }
int32_t newt_wifi_init_no_nvs(void) { return 0; }
void newt_wifi_usb_bbpll(void) {}
int32_t newt_wifi_storage_ram(void) { return 0; }
int32_t newt_wifi_mode_station(void) { return 0; }
int32_t newt_wifi_start(void) { return 0; }
int32_t newt_wifi_stop(void) { return 0; }
int32_t newt_wifi_deinit(void) { return 0; }
int32_t newt_wifi_clear_config(void) { return 0; }

/* ESP_FAIL: the SDK's blocking scan could not run. */
int32_t newt_wifi_scan(void) {
    preview_freeze_at("wifi.scan");
    scans++;
    heard_count = heard_next = 0;
    if (scans == 1 && preview_conf_is("wifi.scan", "failed")) return -1;
    if (scans == 1 && preview_conf_is("wifi.scan", "none")) return 0;
    const char *list = preview_conf("wifi.networks");
    if (!list) return 0;
    long first = preview_conf_long("wifi.first", 16);
    long limit = first + (long)scans - 1;
    char copy[1024];
    strncpy(copy, list, sizeof copy - 1);
    copy[sizeof copy - 1] = 0;
    for (char *save = NULL, *entry = strtok_r(copy, ";", &save); entry && heard_count < 16
         && (long)heard_count < limit; entry = strtok_r(NULL, ";", &save)) {
        char *open = strchr(entry, ','), *name = open ? strchr(open + 1, ',') : NULL;
        if (!name) continue;
        heard[heard_count].rssi = (int32_t)strtol(entry, NULL, 10);
        heard[heard_count].open = open[1] == '1';
        strncpy(heard[heard_count].name, name + 1, 32);
        heard[heard_count].name[32] = 0;
        heard_count++;
    }
    return 0;
}
int32_t newt_wifi_scan_count(uint16_t *count) { *count = (uint16_t)heard_count; return 0; }
int32_t newt_wifi_scan_record(uint8_t *name, int32_t *rssi, uint8_t *open) {
    if (heard_next >= heard_count) return -1;
    memset(name, 0, 33);
    memcpy(name, heard[heard_next].name, strlen(heard[heard_next].name));
    *rssi = heard[heard_next].rssi;
    *open = heard[heard_next].open;
    heard_next++;
    return 0;
}
int32_t newt_wifi_clear_ap_list(void) { heard_count = heard_next = 0; return 0; }

/* Fixed invented entropy: previews repeat, and nothing here is a key claim. */
static uint32_t drawn;
void newt_wifi_radio_draw(uint8_t *fresh) {
    for (int i = 0; i < 32; i++) fresh[i] = (uint8_t)(0x5a + 7 * i + drawn);
    drawn++;
}
uint32_t newt_wifi_random_word(void) { return 0x9e3779b9u * ++drawn; }

/* Invented digest (FNV-style fold), not SHA-256: only the entropy mixer and
 * the sealed record's nonce read it, and both stay inside this host run. */
int32_t newt_wifi_sha256(const uint8_t *input, size_t bytes, uint8_t *output) {
    uint64_t state = 0xcbf29ce484222325ull;
    for (int i = 0; i < 32; i++) {
        for (size_t at = 0; at < bytes; at++) state = (state ^ input[at] ^ (uint64_t)i) * 0x100000001b3ull;
        output[i] = (uint8_t)(state >> 29);
    }
    return 0;
}
/* Invented cipher: seal copies, open copies back. The card record round-trips
 * through the firmware's real Save/restore workflow; it is not a GCM claim. */
int32_t newt_wifi_gcm_seal(const uint8_t *key, const uint8_t *nonce, const uint8_t *header,
                           const uint8_t *plain, uint8_t *cipher, uint8_t *tag) {
    (void)key; (void)nonce; (void)header;
    memcpy(cipher, plain, 98);
    memset(tag, 0x7a, 16);
    return 0;
}
int32_t newt_wifi_gcm_open(const uint8_t *key, const uint8_t *nonce, const uint8_t *header,
                           const uint8_t *cipher, const uint8_t *tag, uint8_t *plain) {
    (void)key; (void)nonce; (void)header;
    for (int i = 0; i < 16; i++) if (tag[i] != 0x7a) return -1;
    memcpy(plain, cipher, 98);
    return 0;
}

/* One join scope; the payload is this static, as wifi.cpp's stack object. */
static struct { uint8_t station, connected, addressed; uint16_t reason; } join;
void newt_wifi_with_join(void *context, void (*callback)(void *, void *)) {
    memset(&join, 0, sizeof join);
    callback(context, &join);
    memset(&join, 0, sizeof join);
}
int32_t newt_wifi_netif_init(void) { return 0; }
uint8_t newt_wifi_join_station(void *scope) { (void)scope; join.station = 1; return 1; }
int32_t newt_wifi_join_register_wifi(void *scope) { (void)scope; return 0; }
int32_t newt_wifi_join_register_ip(void *scope) { (void)scope; return 0; }
void newt_wifi_join_prepare(void *scope, const uint8_t *name, const uint8_t *password, size_t size) {
    (void)scope; (void)name; (void)password; (void)size;
}
int32_t newt_wifi_join_set_config(void *scope) { (void)scope; return 0; }
void newt_wifi_join_wipe_config(void *scope) { (void)scope; }
/* Pinned IDF reasons the firmware classifies: 15 4WAY_HANDSHAKE_TIMEOUT
 * (wrong password), 201 NO_AP_FOUND, 2 AUTH_EXPIRE (other failure). A timeout
 * never addresses, so the firmware waits out its own 20 s on the host clock. */
int32_t newt_wifi_connect(void) {
    preview_freeze_at("wifi.join");
    const char *outcome = preview_conf("wifi.join");
    if (!outcome || strcmp(outcome, "connected") == 0) { join.connected = join.addressed = 1; }
    else if (strcmp(outcome, "wrong-password") == 0) join.reason = 15;
    else if (strcmp(outcome, "not-found") == 0) join.reason = 201;
    else if (strcmp(outcome, "failed") == 0) join.reason = 2;
    else if (strcmp(outcome, "no-address") == 0) join.connected = 1;
    return 0;
}
uint8_t newt_wifi_join_addressed(void *scope) { (void)scope; return join.addressed; }
uint16_t newt_wifi_join_reason(void *scope) { (void)scope; return join.reason; }
uint8_t newt_wifi_join_connected(void *scope) { (void)scope; return join.connected; }
int32_t newt_wifi_disconnect(void) { join.connected = join.addressed = 0; return 0; }
void newt_wifi_join_destroy_station(void *scope) { (void)scope; join.station = 0; }
int32_t newt_wifi_join_unregister_ip(void) { return 0; }
int32_t newt_wifi_join_unregister_wifi(void) { return 0; }

/* The update boot runs the linker's deferred Wi-Fi constructor list; the
 * host links none, so start and end name the same address (an empty list). */
static void no_constructor(void) {}
void (*const preview_wifi_init_list)(void) = no_constructor;
extern void (*const _newt_wifi_init_start)(void) __attribute__((alias("preview_wifi_init_list")));
extern void (*const _newt_wifi_init_end)(void) __attribute__((alias("preview_wifi_init_list")));

/* A reading boot lends the Wi-Fi-only memory range to its heap. The host has
 * no such linker range: lend a small static one and report its size. */
char _newt_wifi_start[4096] __attribute__((aligned(16)));
char _newt_wifi_end;
int32_t newt_wifi_lend_memory(uintptr_t start, uintptr_t end, int32_t *bytes) {
    (void)end;
    *bytes = (int32_t)sizeof _newt_wifi_start;
    return start ? 0 : -1;
}
