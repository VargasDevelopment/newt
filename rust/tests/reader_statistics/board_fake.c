/* SPDX-License-Identifier: MIT
 * Host transport only. Same parse poison as the Board fixture; deterministic
 * UTC snapshots and explicit cold reset never link into firmware. */
#define nb_return_build statistics_return_build
#define nb_clock_snapshot statistics_unused_clock_snapshot
#define nb_clock_read statistics_unused_clock_read
#include "../reader_checkpoint/board_fake.c"
#undef nb_return_build
#undef nb_clock_snapshot
#undef nb_clock_read
static uint8_t statistics_raw[16] = {0, 0, 0x12, 1, 0x31, 0x01, 0x26};
static int statistics_complete = 1;
void statistics_clock(const uint8_t *raw, int complete) {
    memcpy(statistics_raw, raw, sizeof statistics_raw);
    statistics_complete = complete;
}
static int32_t statistics_snapshot(uint8_t *out, size_t len) {
    if (!out) return NB_NULL;
    if (len != sizeof statistics_raw) return NB_LENGTH;
    memcpy(out, statistics_raw, len);
    return statistics_complete ? NB_OK : NB_IO;
}
int32_t nb_clock_snapshot(uint8_t *out, size_t len) { return statistics_snapshot(out, len); }
int32_t nb_clock_read(uint8_t *out, size_t len) { return statistics_snapshot(out, len); }
void statistics_poison(void) {
    assert(!lent);
    memset(pixels, 0x69, sizeof pixels);
}
uint32_t statistics_corners(void) {
    assert(!lent);
    return pixels[0] == 0xff && pixels[98] == 0xff &&
           pixels[sizeof pixels - 99] == 0xff && pixels[sizeof pixels - 1] == 0xff;
}

void nb_return_build(void *display) {
    statistics_return_build(display);
    memset(pixels, 0x69, sizeof pixels);
}

/* Whole logical frame observation; never an alternate firmware buffer. */
void statistics_frame(uint8_t *out, size_t size) {
    assert(!lent && size == sizeof pixels);
    memcpy(out, pixels, size);
}
