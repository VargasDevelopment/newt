/* SPDX-License-Identifier: MIT
 * HOST PREVIEW FAKE ONLY. Input, NVS, serial and card-listing natives for the
 * real firmware, over one preview folder ($NEWT_PREVIEW_DIR):
 *
 *   script.txt    codes still to play: buttons 0-6, 7 (a minute with no press),
 *                 or 8 then a button (pressed before the screen settles)
 *   fixture.conf  key=value lines (fixture.c)
 *   card/         the SD card, as /newt-sd
 *   nvs/NS/KEY    one NVS blob per file
 *   panel.bin     the panel's last native frame (e-ink keeps it over a boot)
 *   frame.pbm     that frame as the firmware's logical picture
 *   receipt.txt   the firmware's own DEV USB replies at the end of the run
 *   exit.txt      why the run ended: idle, sleep, restart or freeze
 *
 * Input runs the firmware's real producer task as a coroutine: a scripted
 * press is one producer poll that sees that button held, made only once the
 * screen has settled (no refresh, press or card write for SETTLE_MS of fake
 * time). After the last press settles, the fake connects as a DEV USB host
 * and asks the firmware for its DIAG, STATUS and FRAME replies, as tools/dev-usb.py
 * does on a device, then ends the run. Nothing here decides what a screen is.
 */
#include <dirent.h>
#include <malloc.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <ucontext.h>
#include <unistd.h>
#include "native_io.h"
#include "fixture.h"

#define SETTLE_MS 4000u
/* Fake time a step may take to settle before the run is a fake bug. */
#define RUN_LIMIT_MS (30u * 60u * 1000u)
#define WAIT_MINUTE 7
#define QUEUED 8

uint32_t nb_millis(void);
void preview_advance(uint32_t ms);
const uint8_t *preview_panel(int *valid);
void preview_save_retained(void);

char *fixture_path(const char *path) {
    static char mapped[4][4096];
    static unsigned next;
    if (strncmp(path, "/newt-sd", 8) != 0) { fprintf(stderr, "path outside the card: %s\n", path); abort(); }
    char *out = mapped[next++ % 4];
    snprintf(out, 4096, "%s%s", preview_path("card"), path + 8);
    return out;
}
void fixture_unexpected(const char *name) {
    fprintf(stderr, "unexpected host peripheral: %s\n", name);
    abort();
}

/* ---- the script ---- */
static uint8_t script[1024];
static size_t script_count, script_next;
static int script_loaded;
static void load_script(void) {
    if (script_loaded) return;
    script_loaded = 1;
    FILE *file = fopen(preview_path("script.txt"), "r");
    unsigned code;
    while (file && script_count < sizeof script && fscanf(file, "%u", &code) == 1) script[script_count++] = (uint8_t)code;
    if (file) fclose(file);
}

/* ---- ending a run ---- */
static void write_file(const char *name, const void *data, size_t length) {
    FILE *file = fopen(preview_path(name), "wb");
    if (!file || fwrite(data, 1, length, file) != length || fclose(file) != 0) { perror(name); exit(70); }
}
/* frame.pbm: the panel as the firmware's logical frame (one is black), in
 * the orientation its last DEV FRAME reply named; portrait when the run ended
 * without one (sleep, restart, freeze), as every system screen is. */
enum { PORTRAIT, LANDSCAPE_CCW, LANDSCAPE_CW };
static int shown = PORTRAIT;
static uint8_t reversed_bits(uint8_t byte) {
    uint8_t out = 0;
    for (int bit = 0; bit < 8; ++bit) if (byte & (1u << bit)) out |= (uint8_t)(0x80u >> bit);
    return out;
}
static void write_frame(const uint8_t *panel) {
    static uint8_t logical[99 * 528];
    const char *header;
    if (shown == LANDSCAPE_CCW) {
        header = "P4\n792 528\n";
        for (size_t i = 0; i < sizeof logical; ++i) logical[i] = (uint8_t)~panel[i];
    } else if (shown == LANDSCAPE_CW) {
        /* frame.rs: clockwise is the native raster turned half a turn. */
        header = "P4\n792 528\n";
        for (size_t i = 0; i < sizeof logical; ++i) logical[i] = (uint8_t)~reversed_bits(panel[sizeof logical - 1 - i]);
    } else {
        header = "P4\n528 792\n";
        memset(logical, 0, sizeof logical);
        /* frame.rs to_native_frame: logical (x, y) is native (y, 527 - x). */
        for (size_t row = 0; row < 528; ++row)
            for (size_t column = 0; column < 792; ++column)
                if (!(panel[row * 99 + column / 8] & (0x80 >> (column % 8)))) {
                    size_t at = column * 528 + (527 - row);
                    logical[at / 8] |= (uint8_t)(0x80 >> (at % 8));
                }
    }
    FILE *file = fopen(preview_path("frame.pbm"), "wb");
    if (!file || fputs(header, file) < 0 || fwrite(logical, 1, sizeof logical, file) != sizeof logical || fclose(file) != 0) {
        perror("frame.pbm");
        exit(70);
    }
}
void preview_finish(int code, const char *reason) {
    int valid;
    const uint8_t *panel = preview_panel(&valid);
    if (valid) { write_file("panel.bin", panel, 99 * 528); write_frame(panel); }
    preview_save_retained();
    FILE *rest = fopen(preview_path("script.txt"), "w");
    for (size_t i = script_next; rest && i < script_count; ++i) fprintf(rest, "%u\n", script[i]);
    if (rest) fclose(rest);
    write_file("exit.txt", reason, strlen(reason));
    fflush(NULL);
    exit(code);
}
/* A worker's freeze point: the panel shows an in-flight page. */
void preview_freeze(void) { preview_finish(0, "freeze"); }

/* ---- the input owner: native atoms, queue and the producer coroutine ---- */
struct event { uint32_t ticket; uint8_t button; };
static struct owner {
    uint32_t atoms[2];
    struct event queue[32];
    size_t head, count;
    uint8_t gate_pending;
} owner;
static ucontext_t ui_context, task_context;
static int task_started, task_suspended, in_task, held = -1;
static uint32_t settled;

static unsigned refreshes, open_back_refreshes;
void preview_settle_reset(void) {
    settled = 0;
    ++refreshes;
    /* "freeze=open.back": the first refresh after open.back's press. */
    if (open_back_refreshes && refreshes > open_back_refreshes) preview_freeze_at("open.back");
}

uint32_t np_owner_with_deferred(uint32_t (*body)(void *, void *), void *argument) { return body(&owner, argument); }
uint8_t np_owner_create_queue(void *o) { (void)o; return 1; }
uint32_t np_owner_with(uint32_t (*body)(void *, void *), void *argument) { return body(&owner, argument); }
void *np_owner_state(void *o) { return ((struct owner *)o)->atoms; }
void *np_owner_dropped(void *o) { return ((struct owner *)o)->atoms + 1; }
void *np_owner_queue(void *o) { return &((struct owner *)o)->queue; }
void *np_owner_gate(void *o) { return &((struct owner *)o)->gate_pending; }
uint8_t np_owner_start(void *o, void (*entry)(void *), void *context) {
    (void)o;
    static uint8_t stack[1 << 20];
    if (getcontext(&task_context)) return 0;
    task_context.uc_stack.ss_sp = stack;
    task_context.uc_stack.ss_size = sizeof stack;
    task_context.uc_link = NULL; /* The producer never returns. */
    makecontext(&task_context, (void (*)(void))entry, 1, context);
    task_started = 1;
    in_task = 1;
    swapcontext(&ui_context, &task_context); /* Its first poll, nothing held. */
    return 1;
}
uint8_t np_owner_suspend(void *o) { (void)o; task_suspended = 1; return 1; }
void np_owner_resume(void *o) { (void)o; task_suspended = 0; }
uint32_t np_atomic_load(uint32_t *a) { return *a; }
uint32_t np_atomic_exchange(uint32_t *a, uint32_t b) { uint32_t old = *a; *a = b; return old; }
uint32_t np_atomic_add(uint32_t *a, uint32_t b) { uint32_t old = *a; *a += b; return old; }
uint32_t np_atomic_sub(uint32_t *a, uint32_t b) { uint32_t old = *a; *a -= b; return old; }
uint32_t np_atomic_or(uint32_t *a, uint32_t b) { uint32_t old = *a; *a |= b; return old; }
uint32_t np_atomic_and(uint32_t *a, uint32_t b) { uint32_t old = *a; *a &= b; return old; }
uint8_t np_atomic_cas_weak(uint32_t *a, uint32_t *expected, uint32_t b) {
    if (*a == *expected) { *a = b; return 1; }
    *expected = *a;
    return 0;
}
uint8_t *np_gate_enter(void *gate) { return gate; }
void np_gate_exit(void *gate) { (void)gate; }

/* The producer's view of the buttons: only the scripted one is held. */
void np_update(void *input) { (void)input; }
uint8_t np_physical_power(void *input) { (void)input; return 0; }
uint8_t np_logical_power(void *input) { (void)input; return 0; }
uint8_t np_pressed(void *input, uint8_t button) { (void)input; return held == button; }
static uint8_t push(struct event event, int front) {
    if (owner.count == 32) return 0;
    if (front) { owner.head = (owner.head + 31) % 32; owner.queue[owner.head] = event; }
    else owner.queue[(owner.head + owner.count) % 32] = event;
    owner.count++;
    return 1;
}
uint8_t np_tail(void *queue, uint8_t button) { (void)queue; return push((struct event){0, button}, 0); }
uint8_t np_front_wait(void *queue, uint8_t button) { (void)queue; return push((struct event){0, button}, 1); }
uint8_t np_send_ticket(void *queue, uint32_t ticket, uint8_t button) { (void)queue; return push((struct event){ticket, button}, 0); }
uint8_t np_receive(void *queue, struct event *event) {
    (void)queue;
    if (!owner.count) return 0;
    *event = owner.queue[owner.head];
    owner.head = (owner.head + 1) % 32;
    owner.count--;
    return 1;
}
uint8_t np_peek(void *queue, struct event *event) {
    (void)queue;
    if (!owner.count) return 0;
    *event = owner.queue[owner.head];
    return 1;
}
uint8_t np_pending(void *queue) { (void)queue; return owner.count != 0; }

/* ---- DEV USB host: HELLO, then DIAG, STATUS and FRAME, as tools/dev-usb.py asks ---- */
static int connected;
static char request[128];
static size_t request_length, request_next;
static char line[1024];
static size_t line_length;
static unsigned long dev_boot, dev_session;
static int dev_step;
static uint32_t observed_at;
static FILE *receipt;
static void ask(const char *text) {
    snprintf(request, sizeof request, "%s\n", text);
    request_length = strlen(request);
    request_next = 0;
}
static void observe(void) {
    connected = 1;
    observed_at = nb_millis();
    receipt = fopen(preview_path("receipt.txt"), "w");
    if (!receipt) { perror("receipt.txt"); exit(70); }
    ask("N1 HELLO");
}
static void reply(const char *text) {
    unsigned long boot, session, id;
    int body = 0;
    if (sscanf(text, "!N1 %lu %lu %lu %n", &boot, &session, &id, &body) != 3 || !body) return;
    const char *checksum = strstr(text + body, " *");
    fprintf(receipt, "%.*s\n", checksum ? (int)(checksum - text - body) : (int)strlen(text + body), text + body);
    static const char *const asks[] = {"DIAG", "STATUS", "FRAME"};
    if (dev_step == 0 && strncmp(text + body, "HELLO ", 6) == 0) {
        dev_boot = boot;
        dev_session = session;
    } else if (dev_step == 0 || id != (unsigned long)dev_step) {
        return;
    }
    if (strncmp(text + body, "FRAME ", 6) == 0)
        shown = strstr(text + body, "orientation=landscape-ccw ") ? LANDSCAPE_CCW
                  : strstr(text + body, "orientation=landscape-cw ") ? LANDSCAPE_CW : PORTRAIT;
    if (dev_step < 3) {
        char next[96];
        snprintf(next, sizeof next, "N1 %lu %lu %d %s", dev_boot, dev_session, dev_step + 1, asks[dev_step]);
        ask(next);
    }
    dev_step++;
}
static FILE *serial_log;
void nsc_begin(void) { serial_log = fopen(preview_path("serial.log"), "ab"); }
void nsc_set_tx_timeout_ms(uint32_t ms) { (void)ms; }
void nsc_flush(void) { if (serial_log) fflush(serial_log); }
void nsc_wake_transmitter(void) {}
static uint32_t cover_usb_resume;
static int cover_usb_work;
static size_t cover_usb_used;
static uint32_t cover_usb_drain;
int32_t nsc_available_for_write(void) {
    if (!preview_conf_long("cover.usb.pause_ms", 0)) return 4096;
    if (nb_millis() < cover_usb_resume) return (int32_t)preview_conf_long("cover.usb.free", 200);
    if (nb_millis() >= cover_usb_drain) cover_usb_used = 0;
    return (int32_t)(256 - cover_usb_used);
}
uint8_t nsc_connected(void) { return (uint8_t)connected; }
int32_t nsc_available(void) { return (int32_t)(request_length - request_next); }
int32_t nsc_read_byte(void) { return request_next < request_length ? (uint8_t)request[request_next++] : -1; }
size_t nsc_write(const uint8_t *data, size_t length) {
    if (preview_conf_long("cover.usb.pause_ms", 0)) {
        size_t free_bytes = (size_t)nsc_available_for_write();
        if (length >= 6 && memcmp(data, "COVER ", 6) == 0 && length > free_bytes) {
            FILE *trace = fopen(preview_path("cover-blocked.txt"), "ab");
            if (trace) { fputs("write\n", trace); fclose(trace); }
            return 0;
        }
        cover_usb_used = length > 256 - cover_usb_used ? 256 : cover_usb_used + length;
        cover_usb_drain = nb_millis() + 30;
    }
    if (serial_log) fwrite(data, 1, length, serial_log);
    for (size_t i = 0; i < length; ++i) {
        if (data[i] == '\n') {
            line[line_length] = 0;
            if (connected && strncmp(line, "!N1 ", 4) == 0) reply(line);
            line_length = 0;
        } else if (line_length + 1 < sizeof line) {
            line[line_length++] = (char)data[i];
        }
    }
    return length;
}

/* ---- time: every wait is fake time, and the idle hook ---- */
static void press(uint8_t button) {
    held = button;
    in_task = 1;
    swapcontext(&ui_context, &task_context);
    held = -1;
}
static uint32_t step_at;
void nb_delay_ms(uint32_t ms) {
    preview_advance(ms ? ms : 1);
    settled += ms ? ms : 1;
    load_script();
    if (nb_millis() - step_at > RUN_LIMIT_MS) { fputs("preview never settled\n", stderr); preview_finish(71, "unsettled"); }
    if (settled < SETTLE_MS || owner.count) return;
    if (script_next < script_count) {
        step_at = nb_millis();
        /* The reader keeps reading time across an idle minute, as on a device. */
        if (script[script_next] == WAIT_MINUTE) { ++script_next; preview_advance(60000); step_at = nb_millis(); return; }
        if (!task_started || task_suspended) { fputs("no input task for a scripted press\n", stderr); preview_finish(72, "no-input"); }
        settled = 0;
        press(script[script_next++]);
        /* A queued press joins this one before the UI runs, so one input drain
         * sees both, as when a reader presses faster than the panel redraws. */
        while (script_next + 1 < script_count && script[script_next] == QUEUED) {
            press(script[script_next + 1]);
            script_next += 2;
        }
        return;
    }
    if (!connected) { observe(); return; }
    /* Two settle windows without every reply: the bridge is not answering. */
    if (dev_step == 4 || nb_millis() - observed_at > 2 * SETTLE_MS) {
        fclose(receipt);
        preview_finish(0, "idle");
    }
}
void np_delay_ms(uint32_t ms) {
    if (!in_task) { nb_delay_ms(ms); return; }
    in_task = 0;
    swapcontext(&task_context, &ui_context);
}

/* ---- NVS: one file per blob under nvs/NAMESPACE/KEY ---- */
#define NVS_NOT_FOUND 0x1102
#define NVS_INVALID_LENGTH 0x110c
#define NVS_FAIL 0x1106
static char namespaces[16][16];
static size_t namespace_count;
static const char *blob_path(uint32_t handle, const char *key) {
    static char path[4096];
    snprintf(path, sizeof path, "%s/%s/%s", preview_path("nvs"), namespaces[handle - 1], key);
    return path;
}
static const char *store_name(const char *operation, uint32_t handle) {
    static char name[64];
    snprintf(name, sizeof name, "%s:%s", operation, namespaces[handle - 1]);
    return name;
}
uint8_t newt_nvs_ready(void) { return 1; }
uint8_t newt_nvs_issue(void) { return 0; }
int32_t newt_nvs_open(const char *name, uint8_t readonly, uint32_t *out) {
    char folder[4096];
    snprintf(folder, sizeof folder, "%s/%s", preview_path("nvs"), name);
    struct stat info;
    if (stat(folder, &info) != 0) {
        if (readonly) return NVS_NOT_FOUND;
        mkdir(preview_path("nvs"), 0700);
        if (mkdir(folder, 0700) != 0) return NVS_FAIL;
    }
    for (size_t i = 0; i < namespace_count; ++i)
        if (strcmp(namespaces[i], name) == 0) { *out = (uint32_t)i + 1; return 0; }
    if (namespace_count == 16 || strlen(name) >= 16) return NVS_FAIL;
    strcpy(namespaces[namespace_count++], name);
    *out = (uint32_t)namespace_count;
    return 0;
}
void newt_nvs_close(uint32_t handle) { (void)handle; }
int32_t newt_nvs_get(uint32_t handle, const char *key, void *out, size_t *length) {
    if (preview_store_fault(store_name("nvs-get", handle)) == 1) return NVS_FAIL;
    FILE *file = fopen(blob_path(handle, key), "rb");
    if (!file) return NVS_NOT_FOUND;
    uint8_t bytes[4096];
    size_t count = fread(bytes, 1, sizeof bytes, file);
    fclose(file);
    if (out) {
        if (*length < count) { *length = count; return NVS_INVALID_LENGTH; }
        memcpy(out, bytes, count);
    }
    *length = count;
    return 0;
}
int32_t newt_nvs_set(uint32_t handle, const char *key, const void *data, size_t length) {
    if (preview_store_fault(store_name("nvs-set", handle)) == 1) return NVS_FAIL;
    settled = 0;
    char temporary[4096];
    snprintf(temporary, sizeof temporary, "%s.tmp", blob_path(handle, key));
    FILE *file = fopen(temporary, "wb");
    if (!file || fwrite(data, 1, length, file) != length || fclose(file) != 0) return NVS_FAIL;
    return rename(temporary, blob_path(handle, key)) == 0 ? 0 : NVS_FAIL;
}
int32_t newt_nvs_commit(uint32_t handle) {
    return preview_store_fault(store_name("nvs-commit", handle)) == 2 ? NVS_FAIL : 0;
}
int32_t newt_nvs_erase(uint32_t handle, const char *key) {
    settled = 0;
    return unlink(blob_path(handle, key)) == 0 ? 0 : NVS_NOT_FOUND;
}
int32_t newt_nvs_visit(uint8_t (*visit)(void *, const char *, const char *, uint8_t), void *context) {
    DIR *spaces = opendir(preview_path("nvs"));
    if (!spaces) return 0;
    struct dirent *space;
    int32_t result = 0;
    while (!result && (space = readdir(spaces))) {
        if (space->d_name[0] == '.') continue;
        char folder[4096];
        snprintf(folder, sizeof folder, "%s/%s", preview_path("nvs"), space->d_name);
        DIR *keys = opendir(folder);
        struct dirent *key;
        while (keys && !result && (key = readdir(keys)))
            if (key->d_name[0] != '.' && !strstr(key->d_name, ".tmp") && !visit(context, space->d_name, key->d_name, 1))
                result = -1;
        if (keys) closedir(keys);
    }
    closedir(spaces);
    return result;
}

/* ---- the card: native_io.c's POSIX calls, relocated, plus these hooks ---- */
static char descriptors[256][4096];
static int cover_heap_dropped;
/* "card.trace=TEXT": each file open or folder listing of a card path holding
 * TEXT appends "open PATH" or "list PATH" to card-trace.txt, so a test can
 * see which card work a boot did. */
static void trace_card(const char *operation, const char *path) {
    const char *trace = preview_conf("card.trace");
    if (!trace || !*trace || !strstr(path, trace)) return;
    FILE *file = fopen(preview_path("card-trace.txt"), "a");
    if (!file) return;
    fprintf(file, "%s %s\n", operation, path);
    fclose(file);
}
void preview_io_list(const char *path) { trace_card("list", path); }
/* "card.change=TEXT": the second open of a card path holding TEXT finds its
 * last byte changed, as if the file were replaced between two reads. */
static int change_opens;
static void change_file(const char *path) {
    const char *change = preview_conf("card.change");
    if (!change || !*change || !strstr(path, change) || ++change_opens != 2) return;
    FILE *file = fopen(fixture_path(path), "r+b");
    if (!file) return;
    if (fseek(file, -1, SEEK_END) == 0) {
        int last = fgetc(file);
        fseek(file, -1, SEEK_END);
        fputc(last ^ 1, file);
    }
    fclose(file);
}
/* "card.edit=PATH" with "card.edit.on=TEXT": the first open of a card path
 * holding TEXT flips the middle byte of PATH and keeps its size and times, as
 * a same-size copy within one FAT time step would. */
static int edit_used;
static void edit_file(const char *path) {
    const char *target = preview_conf("card.edit"), *on = preview_conf("card.edit.on");
    if (edit_used || !target || !*target || !on || !*on || !strstr(path, on)) return;
    edit_used = 1;
    const char *host = fixture_path(target);
    struct stat before;
    FILE *file = stat(host, &before) == 0 ? fopen(host, "r+b") : NULL;
    if (!file) return;
    if (fseek(file, before.st_size / 2, SEEK_SET) == 0) {
        int middle = fgetc(file);
        fseek(file, before.st_size / 2, SEEK_SET);
        fputc(middle ^ 1, file);
    }
    fclose(file);
    utimensat(AT_FDCWD, host, (struct timespec[2]){before.st_atim, before.st_mtim}, 0);
}
void preview_io_open(int32_t fd, const char *path) {
    const char *drop = preview_conf("cover.heap.drop.on");
    if (fd >= 0 && drop && *drop && strstr(path, drop)) cover_heap_dropped = 1;
    cover_usb_work = 1;
    if (fd >= 0 && fd < 256) snprintf(descriptors[fd], sizeof descriptors[fd], "%s", path);
    trace_card("open", path);
    change_file(path);
    edit_file(path);
}
/* "open.back=TEXT", "open.eio=TEXT" or "open.slow=TEXT": once a wake has
 * refreshed the panel (its picture or working page), the first card read of a
 * path holding TEXT presses Back as a hand would while that open still reads
 * the book (Confirm first with "open.confirm=1"), fails that one read with
 * EIO, or takes past the open's 20 s budget, as a stalled card would. */
static int open_fault_used;
static int open_fault(const char *path) {
    if (open_fault_used || !refreshes || preview_boot_kind() != 2) return 0;
    const char *back = preview_conf("open.back"), *eio = preview_conf("open.eio"),
        *slow = preview_conf("open.slow"), *confirm = preview_conf("open.confirm");
    if (eio && *eio && strstr(path, eio)) { open_fault_used = 1; return 1; }
    if (slow && *slow && strstr(path, slow)) { open_fault_used = 1; preview_advance(20001); return 0; }
    if (!back || !*back || !strstr(path, back) || !task_started || task_suspended) return 0;
    open_fault_used = 1;
    open_back_refreshes = refreshes;
    if (confirm && strcmp(confirm, "1") == 0) press(1);
    press(0);
    return 0;
}
/* "card.back=TEXT": the first card read of a path holding TEXT presses Back,
 * as a hand would while that work still reads, on any boot. */
static int card_back_used;
static void card_back(const char *path) {
    const char *text = preview_conf("card.back");
    if (card_back_used || !text || !*text || !strstr(path, text) || !task_started || task_suspended) return;
    card_back_used = 1;
    press(0);
}
/* "card-write:PATH" or "card-sync:PATH": 0 go on, 1 refuse, 2 drop the sync. */
int preview_io_fault(const char *operation, int32_t fd) {
    settled = 0;
    if (strcmp(operation, "card-read") == 0 && fd >= 0 && fd < 256 && open_fault(descriptors[fd])) return 1;
    if (strcmp(operation, "card-read") == 0 && fd >= 0 && fd < 256) card_back(descriptors[fd]);
    char what[4200];
    snprintf(what, sizeof what, "%s:%s", operation, fd >= 0 && fd < 256 ? descriptors[fd] : "");
    return preview_store_fault(what);
}

/* Library folders list in name order, so a preview never depends on the host
 * file system's directory order. A refused "card-list:PATH" fails the open.
 * Under the "library.scan" freeze point each entry takes a second, as on a
 * slow card, and the run freezes once a scan has refreshed the panel. */
struct folder { struct dirent **entries; int count, next; char path[4096]; };
struct folders { struct folder levels[8]; char name[256]; unsigned refreshes; };
static int listed(const struct dirent *entry) { return strcmp(entry->d_name, ".") && strcmp(entry->d_name, ".."); }
uint32_t newt_io_library_scope(uint32_t levels, newt_io_library_scope_callback callback, void *context) {
    if (levels > 8) return 0;
    struct folders folders;
    memset(&folders, 0, sizeof folders);
    folders.refreshes = refreshes;
    return callback(&folders, context);
}
uint32_t newt_io_library_open(void *scope, uint32_t depth, const char *path) {
    struct folders *f = scope;
    if (depth >= 8 || f->levels[depth].entries) return 0;
    char what[4200];
    snprintf(what, sizeof what, "card-list:%s", path);
    if (preview_store_fault(what) == 1) return 0;
    struct folder *level = &f->levels[depth];
    snprintf(level->path, sizeof level->path, "%s", fixture_path(path));
    level->count = scandir(level->path, &level->entries, listed, alphasort);
    level->next = 0;
    if (level->count < 0) { level->entries = NULL; return 0; }
    if (!level->entries) level->entries = malloc(1); /* an empty folder is still open */
    return 1;
}
uint32_t newt_io_library_next(void *scope, uint32_t depth, struct newt_io_library_entry *entry) {
    struct folders *f = scope;
    if (depth >= 8 || !f->levels[depth].entries) return 0;
    if (preview_conf_is("freeze", "library.scan")) {
        if (refreshes != f->refreshes) preview_freeze();
        preview_advance(1000);
    }
    struct folder *level = &f->levels[depth];
    if (level->next == level->count) return 1;
    const char *name = level->entries[level->next++]->d_name;
    char path[8192];
    struct stat s;
    snprintf(path, sizeof path, "%s/%s", level->path, name);
    if (stat(path, &s)) return 0;
    snprintf(f->name, sizeof f->name, "%s", name);
    *entry = (struct newt_io_library_entry){(const uint8_t *)f->name, strlen(f->name), (uint32_t)s.st_size, S_ISDIR(s.st_mode), 0};
    return 2;
}
uint32_t newt_io_library_close(void *scope, uint32_t depth) {
    struct folders *f = scope;
    if (depth >= 8 || !f->levels[depth].entries) return 0;
    struct folder *level = &f->levels[depth];
    for (int i = 0; i < level->count; ++i) free(level->entries[i]);
    free(level->entries);
    level->entries = NULL;
    return 1;
}
/* firmware/page_entry_directory.h's POSIX walker over the preview card: each
 * entry but "." and "..", then the end (a null name); size, time and type
 * only for a sized pass. The census counts on that end. */
int32_t newt_io_walk_pages(const char *folder, uint32_t sized,
                           uint32_t (*visit)(void *, const uint8_t *, size_t, uint64_t, int64_t, uint32_t), void *context) {
    if (!folder || sized > 1 || !visit || !context) return 0;
    char base[4096];
    snprintf(base, sizeof base, "%s", fixture_path(folder));
    DIR *d = opendir(base);
    if (!d) return 0;
    int good = 1;
    for (;;) {
        errno = 0;
        struct dirent *e = readdir(d);
        if (!e && errno) good = 0;
        if (e && (!strcmp(e->d_name, ".") || !strcmp(e->d_name, ".."))) continue;
        uint64_t bytes = 0;
        int64_t time = 0;
        uint32_t regular = 0;
        if (good && e && sized) {
            char path[8192];
            struct stat s;
            snprintf(path, sizeof path, "%s/%s", base, e->d_name);
            if (stat(path, &s)) good = 0;
            else { bytes = (uint64_t)s.st_size; time = s.st_mtime; regular = S_ISREG(s.st_mode) && s.st_size >= 0; }
        }
        good = good && visit(context, e ? (const uint8_t *)e->d_name : NULL, e ? strlen(e->d_name) : 0,
                             bytes, time, regular) == 1;
        if (!e || !good) break;
    }
    return closedir(d) == 0 && good;
}
struct cover_facts { int64_t size; uint64_t inode, device; int64_t seconds; uint64_t links; uint32_t mode; };
/* native/cover.cpp's host branches: this file system's lstat, fstat and rename. */
static int32_t cover_facts(int status, const struct stat *info, struct cover_facts *out) {
    if (status) return errno == EINTR ? 1 : errno == ENOENT ? 2 : errno == EEXIST ? 3 : errno == EIO ? 6 : 5;
    *out = (struct cover_facts){info->st_size, info->st_ino, info->st_dev, info->st_mtime, info->st_nlink, info->st_mode};
    return 0;
}
int32_t newt_cover_stat(const char *path, struct cover_facts *out) {
    struct stat info;
    return cover_facts(lstat(fixture_path(path), &info), &info, out);
}
int32_t newt_cover_fstat(int32_t fd, struct cover_facts *out) {
    struct stat info;
    return cover_facts(fstat(fd, &info), &info, out);
}
_Bool newt_cover_complete_handle_metadata(void) { return 1; }
_Bool newt_cover_rename_replaces_destination(void) { return 1; }
int32_t newt_cover_heap_monitor_start(void) { cover_usb_work = 0; return 0; }
int32_t newt_cover_heap_monitor_stop(void) {
    /* A busy CDC send buffer after cover work; it drains on later UI passes. */
    if (cover_usb_work) cover_usb_resume = nb_millis() + (uint32_t)preview_conf_long("cover.usb.pause_ms", 0);
    return 0;
}
size_t newt_cover_heap_monitor_minimum(void) { return 150000; }
size_t newt_cover_heap_monitor_largest(void) { return 110592; }
uint32_t newt_cover_stack_min_free(void) {
    if (preview_conf_long("cover.usb.pause_ms", 0)) {
        FILE *trace = fopen(preview_path("cover-formats.txt"), "ab");
        if (trace) { fputs("format\n", trace); fclose(trace); }
    }
    return 4096;
}
/* The cover producer's heap, as native/cover.cpp over heap_caps: blocks up
 * to 64 KiB, charged their usable size plus the 16-byte block header. */
_Bool newt_cover_heap_supported(void) { return 1; }
/* "cover.heap.free=BYTES": the free heap cover work sees, so a test can make
 * Memory's live check refuse a large block. */
size_t newt_cover_heap_free_bytes(void) {
    /* A card open consumes heap after commit has begun, above the guard floor
     * but with too little spare room to admit another CRC block. */
    if (cover_heap_dropped) return 68 * 1024;
    const char *free_bytes = preview_conf("cover.heap.free");
    return free_bytes && *free_bytes ? (size_t)strtoul(free_bytes, NULL, 10) : 172032;
}
size_t newt_cover_heap_largest(void) { return 110592; }
size_t newt_cover_heap_charged(void *block) { return block ? malloc_usable_size(block) + 16 : SIZE_MAX; }
void newt_cover_heap_release(void *block) { free(block); }
void *newt_cover_heap_allocate(size_t bytes) {
    void *block = NULL;
    if (!bytes || bytes > 64 * 1024 || posix_memalign(&block, 16, bytes)) return NULL;
    if (newt_cover_heap_charged(block) > bytes + 128) { free(block); return NULL; }
    return block;
}
void nb_running_image_digest(uint8_t *out) { for (size_t i = 0; i < 32; i++) out[i] = (uint8_t)(0x51u + i); }
/* The DEV heap report's walk: this host heap has no blocks to visit. */
void newt_wifi_memory_report_walk(void *context, void (*visit)(void *, size_t, size_t, size_t, size_t, uint8_t)) {
    (void)context;
    (void)visit;
}
