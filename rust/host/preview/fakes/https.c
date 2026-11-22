/* SPDX-License-Identifier: MIT
 * Host fake of the update boot's HTTPS/TLS SDK leaves (native/update_https.cpp).
 * The firmware's own Check (Core's release, manifest and download policy)
 * runs over these. Each URL answers from NEWT_PREVIEW_DIR/http/NAME, where
 * NAME is the URL with every byte outside [A-Za-z0-9.-] made '_'. The file
 * is a status line ("200"), header lines ("Date: ..."), a blank line, then
 * the body. A missing file is an unreachable host.
 * freeze point: http.read, once http.freeze_bytes of one body were read. */
#include "fixture.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void newt_https_note_header(void *context, const char *key, const char *value);

struct client { char name[512]; FILE *file; void *context; int32_t status; long body, length, read; };

void *nh_http_init(const char *url, uint32_t timeout) {
    (void)timeout;
    struct client *client = calloc(1, sizeof *client);
    if (!client) return NULL;
    size_t at = 0;
    for (; url[at] && at + 1 < sizeof client->name; at++) {
        char c = url[at];
        int keep = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '.' || c == '-';
        client->name[at] = keep ? c : '_';
    }
    client->name[at] = 0;
    return client;
}
uint8_t nh_http_set_header(void *client, const char *key, const char *value) {
    (void)client; (void)key; (void)value;
    return 1;
}
uint8_t nh_http_open(void *opaque) {
    struct client *client = opaque;
    char relative[600];
    snprintf(relative, sizeof relative, "http/%s", client->name);
    client->file = fopen(preview_path(relative), "rb");
    if (!client->file) return 0;
    char line[2048];
    if (!fgets(line, sizeof line, client->file)) return 0;
    client->status = (int32_t)strtol(line, NULL, 10);
    return client->status > 0;
}
uint8_t nh_http_set_context(void *opaque, void *context) {
    ((struct client *)opaque)->context = context;
    return 1;
}
/* Headers go to the firmware's own header note, as the SDK event would. */
int64_t nh_http_headers(void *opaque) {
    struct client *client = opaque;
    char line[2048];
    while (fgets(line, sizeof line, client->file)) {
        line[strcspn(line, "\r\n")] = 0;
        if (!line[0]) break;
        char *colon = strchr(line, ':');
        if (!colon) continue;
        *colon = 0;
        const char *value = colon + 1;
        while (*value == ' ') value++;
        if (client->context) newt_https_note_header(client->context, line, value);
    }
    client->body = ftell(client->file);
    fseek(client->file, 0, SEEK_END);
    client->length = ftell(client->file) - client->body;
    fseek(client->file, client->body, SEEK_SET);
    return client->length;
}
int32_t nh_http_status(void *opaque) { return ((struct client *)opaque)->status; }
int32_t nh_http_read(void *opaque, uint8_t *bytes, int32_t count) {
    struct client *client = opaque;
    long freeze = preview_conf_long("http.freeze_bytes", -1);
    if (freeze >= 0 && client->read >= freeze) preview_freeze_at("http.read");
    size_t got = fread(bytes, 1, (size_t)count, client->file);
    client->read += (long)got;
    return (int32_t)got;
}
uint8_t nh_http_complete(void *opaque) {
    struct client *client = opaque;
    return client->file && feof(client->file);
}
void nh_http_close(void *opaque) {
    struct client *client = opaque;
    if (client->file) fclose(client->file);
    client->file = NULL;
}
void nh_http_cleanup(void *opaque) { free(opaque); }

/* The sweep timer never fires on the host; a live handle is enough. */
static int timer;
void *nh_timer_create(void (*callback)(void *), void *deadline) { (void)callback; (void)deadline; return &timer; }
uint8_t nh_timer_start(void *handle, uint64_t interval) { (void)handle; (void)interval; return 1; }
void nh_timer_stop(void *handle) { (void)handle; }
void nh_timer_delete(void *handle) { (void)handle; }
int32_t nh_socket_first(void) { return 0; }
int32_t nh_socket_count(void) { return 0; }
void nh_socket_shutdown(int32_t fd) { (void)fd; }
void nh_socket_sem_deinit(void) {}

/* The TLS heap inside the lent frame: the host SDK never allocates from it,
 * so the firmware's reclaim always finds it empty and the frame comes back. */
size_t nh_tls_header_bytes(void) { return 64; }
void nh_tls_construct(void *header) { memset(header, 0, 64); }
uint8_t nh_tls_register(void *header, uint8_t *start, size_t bytes) { (void)header; return start && bytes; }
void nh_tls_bind_task_lock(void *header) { (void)header; }
uint8_t nh_tls_owner_is_current(void *header) { (void)header; return 0; }
void *nh_tls_malloc(void *header, size_t bytes) { (void)header; (void)bytes; return NULL; }
void nh_tls_free(void *header, void *bytes) { (void)header; (void)bytes; }
void nh_tls_info(void *header, size_t *allocated, size_t *lowest) { (void)header; *allocated = 0; *lowest = 40000; }
int32_t nh_tls_install(void *(*allocate)(size_t, size_t), void (*release)(void *)) { (void)allocate; (void)release; return 0; }
int32_t nh_tls_restore(void) { return 0; }
void *nh_tls_default_calloc(size_t count, size_t size) { return calloc(count, size); }
void nh_tls_default_free(void *bytes) { free(bytes); }
