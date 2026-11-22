/* SPDX-License-Identifier: MIT */
#ifndef NEWT_INPUT_ATOMIC_BRIDGE_H
#define NEWT_INPUT_ATOMIC_BRIDGE_H
#include <stdint.h>
#ifdef __cplusplus
#include <atomic>
/* Foreign owner allocates two words (state,dropped), zero-initialized before
 * either task starts. Rust must NEVER assume std::atomic layout or field offsets.
 * No Newt masks, input tags or clock policy enter this primitive. */
struct np_atomic_u32 { std::atomic<uint32_t> value{0}; };
extern "C" {
#else
struct np_atomic_u32;
#endif
/* Every operation uses memory_order_seq_cst, matching pinned BookOpenControl.
 * Pointers valid/aligned/live for the native atomic object; no plain-memory
 * aliases. fetch operations return PREVIOUS value; add/sub wrap uint32_t.
 * CAS weak updates *expected on failure, permits spurious failure, writes
 * desired on success. Caller retries/decides; ABI retains no pointers. */
uint32_t np_atomic_load(const struct np_atomic_u32 *);
uint32_t np_atomic_or(struct np_atomic_u32 *, uint32_t);
uint32_t np_atomic_and(struct np_atomic_u32 *, uint32_t);
uint32_t np_atomic_add(struct np_atomic_u32 *, uint32_t);
uint32_t np_atomic_sub(struct np_atomic_u32 *, uint32_t);
uint32_t np_atomic_exchange(struct np_atomic_u32 *, uint32_t);
uint8_t np_atomic_cas_weak(struct np_atomic_u32 *, uint32_t *expected, uint32_t desired);
#ifdef __cplusplus
}
#endif
#endif
