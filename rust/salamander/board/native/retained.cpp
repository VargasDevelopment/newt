// SPDX-License-Identifier: MIT
#include "retained.h"
#include <esp_attr.h>
namespace {
// Frozen main.cpp's primitive RTC owners, relocated only in the Y image.
// There is no C++ application user and no second copy of either object.
RTC_NOINIT_ATTR uint32_t sleepWords[17];
RTC_NOINIT_ATTR uint32_t sdResumeWords[35];
static_assert(sizeof(sleepWords) == 68 && sizeof(sdResumeWords) == 140);
}

int32_t nb_retained_words(struct nb_retained_regions *out) {
    if (!out) return NB_NULL;
    *out = {};
    *out = {sleepWords, 17, sdResumeWords, 35};
    return NB_OK;
}
