/* SPDX-License-Identifier: MIT */
#ifndef NEWT_INPUT_POLICY_BRIDGE_H
#define NEWT_INPUT_POLICY_BRIDGE_H
#include <stdint.h>
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Private ABI, not SDK layout. Valid non-null pointers and lifetime are caller
 * obligations. input=InputManager*, queue=QueueHandle_t, gate=np_dev_gate*
 * in DEV only. One task owns input; UI owns receive. Shared queue/gate synchronize.
 * Fixed X3-only/no-touch SDK build; default-null button hook is never installed
 * or mutated. ADC configuration/ACTIVE remain fixed; only task calls InputManager.
 * No task Wire/SPI/SD/display access, including through hooks or touch backends.
 * UI retains Wire (clock/gauge) and SPI/SD. Shutdown requires task death.
 * No buffer/pointer retention, callbacks, parser dispatch or display/SD calls.
 * No exceptions may cross ABI. Queue representation follows NEWT_DEV_USB. */
struct np_event { uint32_t ticket; uint8_t button; };
void np_update(void *input);
uint8_t np_physical_power(void *input);
uint8_t np_logical_power(void *input);
uint8_t np_pressed(void *input, uint8_t button);
/* Y DEV only: same gate backing, a scoped loan under the existing portMUX.
 * The Rust caller must release without SDK I/O, wait, callback or reentry. */
bool *np_gate_enter(void *gate);
void np_gate_exit(void *gate);
uint8_t np_tail(void *queue, uint8_t button);
#if NEWT_DEV_USB
uint8_t np_send_ticket(void *queue, uint32_t ticket, uint8_t button);
#endif
uint8_t np_front_wait(void *queue, uint8_t button);
uint8_t np_receive(void *queue, struct np_event *out);
uint8_t np_peek(void *queue, struct np_event *out);
uint8_t np_pending(void *queue);
void np_delay_ms(uint32_t ms);
#ifdef __cplusplus
}
#endif
#endif
