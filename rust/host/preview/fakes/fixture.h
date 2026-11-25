/* SPDX-License-Identifier: MIT
 * Host preview fixture: one run directory (NEWT_PREVIEW_DIR) that survives
 * the preview's simulated restarts, and its fixture.conf (key=value lines).
 * Fakes read their outcomes here; none decides a screen or a policy. */
#ifndef NEWT_PREVIEW_FIXTURE_H
#define NEWT_PREVIEW_FIXTURE_H
#include <stddef.h>
/* The value for key, or NULL. */
const char *preview_conf(const char *key);
/* True when key's value is exactly value. */
int preview_conf_is(const char *key, const char *value);
long preview_conf_long(const char *key, long fallback);
/* NEWT_PREVIEW_DIR/relative, in a rotating static buffer. */
const char *preview_path(const char *relative);
/* A frozen in-flight frame: when fixture `freeze` names `point`, the driver
 * keeps the frame on the panel now and ends the run (fixture.c: weak default exits 0). */
void preview_freeze_at(const char *point);
/* The driver's frame capture; it does not return. */
void preview_freeze(void);
/* This boot (board.c): 0 power-on, 1 software, 2 deep-sleep (a wake). */
int preview_boot_kind(void);
/* 0 ok, 1 refuse, 2 drop the sync/commit, for "OP:TARGET" (see fixture.c). */
int preview_store_fault(const char *what);
int preview_store_fault_at(const char *what, size_t step);
#endif
