/* SPDX-License-Identifier: MIT
 * fixture.conf reader and the store-fault hook the NVS/card fakes call. */
#include "fixture.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { ENTRIES = 64, TEXT = 1024 };
static char keys[ENTRIES][64], values[ENTRIES][TEXT];
static int entries = -1;

static void load(void) {
    if (entries >= 0) return;
    entries = 0;
    FILE *file = fopen(preview_path("fixture.conf"), "r");
    if (!file) return;
    char line[TEXT + 64];
    while (fgets(line, sizeof line, file) && entries < ENTRIES) {
        line[strcspn(line, "\r\n")] = 0;
        char *equals = strchr(line, '=');
        if (!equals || line[0] == '#') continue;
        *equals = 0;
        snprintf(keys[entries], sizeof keys[entries], "%s", line);
        snprintf(values[entries], sizeof values[entries], "%s", equals + 1);
        entries++;
    }
    fclose(file);
}

const char *preview_conf(const char *key) {
    load();
    for (int i = 0; i < entries; i++) if (strcmp(keys[i], key) == 0) return values[i];
    return NULL;
}
int preview_conf_is(const char *key, const char *value) {
    const char *found = preview_conf(key);
    return found && strcmp(found, value) == 0;
}
long preview_conf_long(const char *key, long fallback) {
    const char *found = preview_conf(key);
    return found ? strtol(found, NULL, 10) : fallback;
}
const char *preview_path(const char *relative) {
    static char paths[4][4096];
    static unsigned next;
    const char *dir = getenv("NEWT_PREVIEW_DIR");
    assert(dir && "NEWT_PREVIEW_DIR names the run directory");
    char *out = paths[next++ % 4];
    snprintf(out, sizeof paths[0], "%s/%s", dir, relative);
    return out;
}

/* The driver's frame capture overrides this; alone, end the run cleanly. */
__attribute__((weak)) void preview_freeze(void) { exit(0); }
void preview_freeze_at(const char *point) {
    if (preview_conf_is("freeze", point)) preview_freeze();
}

/* Store outcomes for the NVS and card fakes. `what` is "OP:TARGET": OP is
 * nvs-get, nvs-set, nvs-commit, card-read, card-write, card-sync or
 * card-list; TARGET the namespace or card path. store.refuse / store.drop
 * hold a substring of `what`: refuse fails the call (nothing moved, so "not
 * saved"); drop lets the bytes land but fails the sync or commit (an
 * unconfirmed commit). 0 ok, 1 refuse, 2 drop. */
int preview_store_fault(const char *what) {
    const char *refuse = preview_conf("store.refuse"), *drop = preview_conf("store.drop");
    if (refuse && *refuse && strstr(what, refuse)) return 1;
    if (drop && *drop && strstr(what, drop)) return 2;
    return 0;
}
