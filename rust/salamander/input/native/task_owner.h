/* SPDX-License-Identifier: MIT */
#ifndef NEWT_NATIVE_INPUT_TASK_OWNER_H
#define NEWT_NATIVE_INPUT_TASK_OWNER_H
#include <stdint.h>
#ifdef __cplusplus
#define NP_NOEXCEPT noexcept
extern "C" {
#else
#define NP_NOEXCEPT
#endif
struct np_atomic_u32;
/* All owner pointers are private lexical capabilities, never stored globally.
 * body may return ONLY before successful np_owner_start. Once started, both
 * Rust and C++ scopes must remain active until whole-program abort/reset.
 * No exceptions/unwinding, external task deletion/suspension, queue deletion,
 * gate reset, counter reset, or duplicate body/start entry is permitted.
 * Sole-UI scoped suspension uses the pair below without ending any owner. */
typedef uint32_t (*np_owner_body)(void *owner, void *argument);
typedef void (*np_task_entry)(void *context);
uint32_t np_owner_with(np_owner_body body, void *argument) NP_NOEXCEPT;
/* Same scope, with no queue yet. Rust restores durable/RTC state first, then
 * begins input and creates exactly the existing queue before task launch. */
uint32_t np_owner_with_deferred(np_owner_body body, void *argument) NP_NOEXCEPT;
uint8_t np_owner_create_queue(void *owner) NP_NOEXCEPT;
void *np_owner_queue(void *owner) NP_NOEXCEPT;
void *np_owner_gate(void *owner) NP_NOEXCEPT;
struct np_atomic_u32 *np_owner_state(void *owner) NP_NOEXCEPT;
struct np_atomic_u32 *np_owner_dropped(void *owner) NP_NOEXCEPT;
uint8_t np_owner_start(void *owner, np_task_entry entry, void *context) NP_NOEXCEPT;
/* Sole UI, scheduling enabled, outside ISR/critical/native callbacks. Failed
 * suspend acquired nothing. Resume only an acquired scope, before committed
 * wake/power changes; committed entry/rejection restart retains that scope. */
uint8_t np_owner_suspend(void *owner) NP_NOEXCEPT;
void np_owner_resume(void *owner) NP_NOEXCEPT;
/* Host-fixture ABI for the non-app Rust policy. Firmware owns this gate in Rust. */
uint8_t np_owner_consume(void *owner, void *bridge, uint32_t ticket,
                         uint8_t connected, uint32_t now) NP_NOEXCEPT;
void np_owner_drain(void *owner, void *bridge) NP_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#undef NP_NOEXCEPT
#endif
