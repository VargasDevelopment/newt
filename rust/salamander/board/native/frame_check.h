/* SPDX-License-Identifier: MIT */
#ifndef NEWT_C3_FRAME_CHECK_H
#define NEWT_C3_FRAME_CHECK_H
#include "board.h"
/* Shared by real bridge and host ABI checks, never dereferences frame bytes. */
static inline int32_t nb_check_frame(const struct nb_frame *f) {
    if (!f) return NB_NULL;
    if (!f->data || f->width != 792 || f->height != 528 ||
        f->stride != 99 || f->len != (size_t)99 * 528) return NB_FRAME;
    return NB_OK;
}
#endif
