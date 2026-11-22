/* SPDX-License-Identifier: MIT */
#ifndef NEWT_C3_RETAINED_H
#define NEWT_C3_RETAINED_H
#include "board.h"
#ifdef __cplusplus
extern "C" {
#endif
struct nb_retained_regions {
    uint32_t *sleep;
    size_t sleep_words;
    uint32_t *sd_resume;
    size_t sd_resume_words;
};
/* Y only: same two RTC_NOINIT objects as frozen main.cpp, 68 and 140 bytes.
 * Native does not interpret, clear or copy a record. The sole Board owner
 * excludes every other task/native/interrupt access for each short loan.
 * All words are initialized ordinary retained RAM (arbitrary on a cold boot),
 * not MMIO. Rust validates raw words before interpreting record fields.
 * Native retains no output pointer. No foreign unwind or asynchronous access.
 * The C++ application main and its RTC owners MUST NOT link into the Y image.
 */
int32_t nb_retained_words(struct nb_retained_regions *out);
#ifdef __cplusplus
}
static_assert(sizeof(nb_retained_regions) == 4 * sizeof(size_t));
static_assert(offsetof(nb_retained_regions, sd_resume) == 2 * sizeof(size_t));
#endif
#endif
