/* SPDX-License-Identifier: MIT
 * HOST PREVIEW FAKE ONLY. The X3 board natives the real firmware calls, over
 * one preview folder ($NEWT_PREVIEW_DIR). Never compiled into firmware.
 *
 * The panel keeps the last refreshed native frame; boot facts, the clock and
 * battery come from files the driver (tools/rust_preview.py) writes; retained
 * words survive a restart or a sleep through retained.bin, as RTC memory does.
 * Sleep and restart end this process; the driver relaunches it as the next boot.
 */
#include "board.h"
#include "hardware.h"
#include "frame_check.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fixture.h"

/* ports.c */
void preview_settle_reset(void);
void preview_finish(int code, const char *reason);

static uint8_t pixels[99 * 528]; /* The panel's native frame buffer. */
static uint8_t shown[99 * 528];  /* What the last refresh put on glass. */
static int lent, refreshed;
static uint32_t millis_value = 1000;

/* Every refresh is a capture point: the preview shows the last one. */
const uint8_t *preview_panel(int *valid) { *valid = refreshed; return shown; }
void preview_advance(uint32_t ms) { millis_value += ms; }

int32_t nb_select_x3(void *d) { return d ? NB_OK : NB_NULL; }
void nb_release_sd(void) {}
int32_t nb_probe_panel(struct nb_probe *p) {
    if (!p) return NB_NULL;
    *p = (struct nb_probe){{0xa1, 0xb2, 0xc3}, 0xfe, 1, 0, 0};
    return NB_OK;
}
/* E-ink keeps its picture without power: a boot starts with the last one. */
int32_t nb_begin(void *d) {
    if (!d) return NB_NULL;
    FILE *file = fopen(preview_path("panel.bin"), "rb");
    if (file) { refreshed = fread(shown, sizeof shown, 1, file) == 1; fclose(file); }
    return NB_OK;
}
int32_t nb_mount_sd(void) {
    if (preview_conf_is("card.mount.trace", "1")) {
        FILE *file = fopen(preview_path("mounts.txt"), "a");
        if (file) { fputs("mount\n", file); fclose(file); }
    }
    return preview_conf_is("card.absent", "1") ? NB_IO : NB_OK;
}
int32_t nb_input_begin(void *i) { return i ? NB_OK : NB_NULL; }
int32_t nb_frame(void *d, struct nb_frame *out) {
    if (!out) return NB_NULL;
    *out = (struct nb_frame){0};
    if (!d || lent) return NB_NULL;
    *out = (struct nb_frame){pixels, sizeof pixels, 792, 528, 99};
    return NB_OK;
}
int32_t nb_lend_build(void *d, struct nb_storage *out) {
    if (!out) return NB_NULL;
    *out = (struct nb_storage){0};
    if (!d || lent) return NB_NULL;
    lent = 1;
    *out = (struct nb_storage){pixels, sizeof pixels};
    return NB_OK;
}
void nb_return_build(void *d) {
    (void)d;
    memset(pixels, 0xff, sizeof pixels);
    lent = 0;
}
int32_t nb_update(void *d, uint32_t mode, uint32_t off) {
    if (!d || lent) return NB_NULL;
    if (mode > 2 || off > 1) return NB_MODE;
    if (preview_conf_long("cover.usb.pause_ms", 0))
        preview_advance((uint32_t)preview_conf_long("cover.paint_ms", 400));
    memcpy(shown, pixels, sizeof shown);
    refreshed = 1;
    preview_settle_reset();
    return NB_OK;
}
int32_t nb_panel_sleep(void *d) { return d ? NB_OK : NB_NULL; }
/* This host panel is binary-only, so grayscale takes its real fallback. */
int32_t nb_gray_encoding(void *display, uint32_t mode, uint32_t *out) {
    if (!display || !out || mode > 2) return NB_NULL;
    *out = 0;
    return NB_OK;
}

uint32_t nb_millis(void) { return millis_value; }
uint32_t nb_micros(void) { return millis_value * 1000u; }
void nb_delay_ms(uint32_t ms);  /* ports.c: the idle hook. */

/* The driver's DS3231 registers (16 bytes, UTC), frozen for the whole run so
 * a preview never depends on how long its fake time ran. */
static int32_t clock_bytes(uint8_t *out, size_t len) {
    if (!out) return NB_NULL;
    if (len != 16) return NB_LENGTH;
    memset(out, 0, len);
    FILE *file = fopen(preview_path("clock.bin"), "rb");
    if (!file) return NB_IO;
    size_t got = fread(out, 1, len, file);
    fclose(file);
    return got == len ? NB_OK : NB_IO;
}
int32_t nb_clock_read(uint8_t *out, size_t len) { return clock_bytes(out, len); }
int32_t nb_clock_snapshot(uint8_t *out, size_t len) { return clock_bytes(out, len); }
/* A set clock (the online check's Date) lands in the same registers. */
uint8_t nb_clock_write(uint8_t first, const uint8_t *bytes, size_t count) {
    uint8_t registers[16];
    if (first + count > sizeof registers) return 0;
    if (clock_bytes(registers, sizeof registers) != NB_OK) memset(registers, 0, sizeof registers);
    memcpy(registers + first, bytes, count);
    FILE *file = fopen(preview_path("clock.bin"), "wb");
    if (!file) return 0;
    int written = fwrite(registers, sizeof registers, 1, file) == 1;
    return (uint8_t)((fclose(file) == 0) & written);
}

/* boot.txt names this boot: power-on (default), software, or deep-sleep. */
int preview_boot_kind(void) {
    char kind[16] = {0};
    FILE *file = fopen(preview_path("boot.txt"), "r");
    if (file) { if (!fgets(kind, sizeof kind, file)) kind[0] = 0; fclose(file); }
    return strncmp(kind, "software", 8) == 0 ? 1 : strncmp(kind, "deep-sleep", 10) == 0 ? 2 : 0;
}
int32_t nb_boot(struct nb_boot *out) {
    if (!out) return NB_NULL;
    switch (preview_boot_kind()) {
    case 1: *out = (struct nb_boot){3, 0, 0, 0}; break;  /* ESP_RST_SW */
    case 2: *out = (struct nb_boot){8, 7, 0, 1}; break;  /* ESP_RST_DEEPSLEEP, GPIO wake */
    default: *out = (struct nb_boot){1, 0, 1, 0}; break; /* ESP_RST_POWERON */
    }
    return NB_OK;
}

/* RTC memory: zero after power-on, kept across restart and deep sleep. */
struct retained_regions { uint32_t *sleep; size_t sleep_words; uint32_t *sd; size_t sd_words; };
static uint32_t retained[17 + 35];
static int retained_loaded;
int32_t nb_retained_words(struct retained_regions *out) {
    if (!retained_loaded) {
        retained_loaded = 1;
        FILE *file = preview_boot_kind() ? fopen(preview_path("retained.bin"), "rb") : NULL;
        if (file) {
            if (fread(retained, sizeof retained, 1, file) != 1) memset(retained, 0, sizeof retained);
            fclose(file);
        }
    }
    *out = (struct retained_regions){retained, 17, retained + 17, 35};
    return NB_OK;
}
void preview_save_retained(void) {
    FILE *file = fopen(preview_path("retained.bin"), "wb");
    if (!file || fwrite(retained, sizeof retained, 1, file) != 1) { perror("retained.bin"); exit(70); }
    fclose(file);
}

void nb_wait_power_release(void) {}
uint32_t nb_arm_power_wake(void) { return 1; }
void nb_power_down_rails(void) {}
/* Battery sleep is a power-off on the X3; the driver's "wake" is the next boot. */
int32_t nb_try_sleep(void) { preview_finish(0, "sleep"); return 0; }
void nb_restart(void) { preview_finish(75, "restart"); }

/* --telemetry sample is an invented battery; unknown reads nothing. */
int32_t nb_hw_read_battery(struct nb_hw_battery *out) {
    if (!out) return NB_HW_NULL;
    *out = (struct nb_hw_battery){0};
    if (preview_conf_is("telemetry", "sample"))
        *out = (struct nb_hw_battery){73, 3990, 1, 1, 1, 0};
    return NB_HW_OK;
}
int32_t nb_hw_free_heap(uint32_t *out) { *out = 172032; return NB_HW_OK; }
int32_t nb_hw_min_free_heap(uint32_t *out) { *out = 150000; return NB_HW_OK; }
int32_t nb_hw_largest_heap_block(uint32_t *out) { *out = 110592; return NB_HW_OK; }
int32_t nb_hw_current_task_stack_low(uint32_t *out) { *out = 4096; return NB_HW_OK; }
int32_t nb_hw_factory_mac(struct nb_hw_factory_mac *out) {
    *out = (struct nb_hw_factory_mac){0, {0x02, 0x4e, 0x45, 0x57, 0x54, 0x01}};
    return NB_HW_OK;
}
int32_t nb_hw_random_word(uint32_t *out) { *out = 42; return NB_HW_OK; }
/* Console gates: the host serial stays one log either way. */
void nb_allow_reading_crash_output(void) {}
void nb_withhold_console(void) {}
