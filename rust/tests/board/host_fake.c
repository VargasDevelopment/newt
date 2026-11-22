/* SPDX-License-Identifier: MIT
 * HOST FAKE ABI ONLY. Never compile into firmware; no SDK headers/stubs.
 * Exercises real Rust wrapper + shared wire validation, not board.cpp/drivers.
 */
#include "board.h"
#include "frame_check.h"
#include <assert.h>
#include <string.h>
static uint8_t pixels[99 * 528]; /* Host fixture, not a target second frame. */
static uint32_t fault, updates, last_mode, last_off, lends, returns, mounts, input_begins;
static int lent;
static uint32_t millis_value = UINT32_MAX;
void fixture_clock_set(uint32_t value) { millis_value = value; }
uint32_t fixture_lends(void) { return lends; }
uint32_t fixture_returns(void) { return returns; }
uint32_t fixture_live(void) { return (uint32_t)lent; }
int32_t nb_lend_build(void *d, struct nb_storage *out) {
    if (!out) return NB_NULL;
    *out = (struct nb_storage){0};
    if (!d) return NB_NULL;
    assert(!lent);
    if (fault == 20) return NB_IO;
    lent = 1; lends++;
    *out = (struct nb_storage){pixels, sizeof pixels};
    if (fault == 21) out->data = NULL;
    if (fault == 22) out->len--;
    if (fault == 23) out->len++;
    return NB_OK;
}
void nb_return_build(void *d) {
    assert(d && lent);
    memset(pixels, 0xff, sizeof pixels);
    lent = 0; returns++;
}
void fixture_fault(uint32_t value) { fault = value; }
uint32_t fixture_updates(void) { return updates; }
uint32_t fixture_mode(void) { return last_mode; }
uint32_t fixture_off(void) { return last_off; }
void fixture_checks(void) {
    struct nb_frame f = {pixels, sizeof pixels, 792, 528, 99};
    assert(nb_check_frame(NULL) == NB_NULL);
    assert(nb_check_frame(&f) == NB_OK);
    f.data = NULL; assert(nb_check_frame(&f) == NB_FRAME); f.data = pixels;
    f.len--; assert(nb_check_frame(&f) == NB_FRAME); f.len++;
    f.len++; assert(nb_check_frame(&f) == NB_FRAME); f.len--;
    f.stride--; assert(nb_check_frame(&f) == NB_FRAME); f.stride++;
    f.width--; assert(nb_check_frame(&f) == NB_FRAME); f.width++;
    f.height--; assert(nb_check_frame(&f) == NB_FRAME);
    f.data = pixels; f.len = sizeof pixels; f.width = 792; f.height = 528;
    assert(nb_frame(NULL, &f) == NB_NULL && !f.data && !f.len);
    assert(nb_frame(pixels, NULL) == NB_NULL);
    assert(nb_update(pixels, 3, 0) == NB_MODE);
    assert(nb_update(pixels, 0, 2) == NB_MODE);
    assert(nb_boot(NULL) == NB_NULL);
    uint8_t clock[17];
    assert(nb_clock_read(NULL, 16) == NB_NULL);
    assert(nb_clock_read(clock, 15) == NB_LENGTH);
    assert(nb_clock_read(clock, 17) == NB_LENGTH);
    assert(nb_clock_snapshot(NULL, 16) == NB_NULL);
    assert(nb_clock_snapshot(clock, 15) == NB_LENGTH);
    assert(nb_clock_snapshot(clock, 17) == NB_LENGTH);
}
int32_t nb_select_x3(void *d) { return d ? NB_OK : NB_NULL; }
void nb_release_sd(void) {}
uint32_t fixture_mounts(void) { return mounts; }
uint32_t fixture_input_begins(void) { return input_begins; }
int32_t nb_mount_sd(void) {
    mounts++;
    return fault == 24 ? NB_IO : fault == 25 ? NB_MODE : fault == 26 ? -781 : NB_OK;
}
int32_t nb_probe_panel(struct nb_probe *p) {
    if (!p) return NB_NULL;
    *p = (struct nb_probe){{0xa1, 0xb2, 0xc3}, 0xfe, 1, 1, 0};
    return NB_OK;
}
int32_t nb_begin(void *d) { return !d ? NB_NULL : fault == 1 ? NB_FRAME : NB_OK; }
int32_t nb_frame(void *d, struct nb_frame *out) {
    assert(!lent);
    if (!out) return NB_NULL;
    *out = (struct nb_frame){0};
    if (!d) return NB_NULL;
    *out = (struct nb_frame){pixels, sizeof pixels, 792, 528, 99};
    /* Deliberately malformed success responses test Rust's independent checks. */
    if (fault == 2) out->data = NULL;
    if (fault == 3) out->len--;
    if (fault == 4) out->len++;
    if (fault == 5) out->width--;
    if (fault == 6) out->height--;
    if (fault == 7) out->stride--;
    return fault == 8 ? -777 : NB_OK;
}
int32_t nb_update(void *d, uint32_t mode, uint32_t off) {
    if (!d) return NB_NULL;
    assert(!lent);
    if (mode > 2 || off > 1) return NB_MODE;
    updates++; last_mode = mode; last_off = off;
    return fault == 9 ? -778 : NB_OK;
}
int32_t nb_panel_sleep(void *d) { return d ? NB_OK : NB_NULL; }
int32_t nb_input_begin(void *i) {
    if (!i) return NB_NULL;
    input_begins++;
    return NB_OK;
}
uint32_t nb_millis(void) { return millis_value; }
uint32_t nb_micros(void) { return 0x12345678; }
/* Fault 12 fails before any byte, 13 after three; only a snapshot keeps them.
 * The real transaction is checked over FakeWire in clock/. */
static int32_t clock_fake(uint8_t *out, size_t len, int keep_prefix) {
    if (!out) return NB_NULL;
    if (len != 16) return NB_LENGTH;
    const size_t got = fault == 12 ? 0 : fault == 13 ? 3 : len;
    for (size_t i = 0; i < len; ++i)
        out[i] = i < got && (got == len || keep_prefix) ? (uint8_t)(i + 0x80) : 0;
    return got == len ? NB_OK : NB_IO;
}
int32_t nb_clock_read(uint8_t *out, size_t len) { return clock_fake(out, len, 0); }
int32_t nb_clock_snapshot(uint8_t *out, size_t len) { return clock_fake(out, len, 1); }
int32_t nb_boot(struct nb_boot *out) {
    if (!out) return NB_NULL;
    *out = (struct nb_boot){0x11223344, 0xaabbccdd, 0, 1}; return NB_OK;
}
void nb_wait_power_release(void) {} /* Host fake has no physical held button. */
uint32_t nb_arm_power_wake(void) { return fault == 11 ? 0 : 1; }
void nb_power_down_rails(void) {}
int32_t nb_try_sleep(void) { return -780; }
void nb_restart(void) {} /* Fake returns to allow host-only scalar API check. */
