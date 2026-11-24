/* SPDX-License-Identifier: MIT */
#ifndef NEWT_C3_SERIAL_CONSOLE_H
#define NEWT_C3_SERIAL_CONSOLE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Private ABI over the SDK's Serial: the C3 USB Serial/JTAG HWCDC that
 * ARDUINO_USB_MODE=1 and ARDUINO_USB_CDC_ON_BOOT=1 select (platformio.ini at
 * bc9ffda5, framework-arduinoespressif32 3.3.7). Sole caller:
 * rust/firmware/serial_console.rs. Each entry makes at most one synchronous
 * SDK call, except DEV begin also requests its receive queue before begin.
 * No retry, callback or retained caller pointer. */

/* Serial.begin(115200), as firmware/main.cpp setup. Once, before any write. */
void nsc_begin(void);
/* Serial.setTxTimeoutMs(ms). The SDK keeps one global value: it bounds the
 * transmit lock wait and each blocked ring-buffer send, for every Serial user. */
void nsc_set_tx_timeout_ms(uint32_t ms);
/* One Serial.write(data, len); returns the SDK's count. A null pointer or zero
 * length returns 0 without an SDK call. The count is the SDK's report, not
 * delivery: 0 also means the lock timed out or Serial was never begun, and
 * after a send timeout HWCDC may still queue the rest under its drop-oldest
 * policy. Never retried here. */
size_t nsc_write(const uint8_t *data, size_t len);
/* DEV channel leaves connection/space/chunk policy on the Rust side. Each
 * entry forwards the existing UsbChannel SDK operation, retaining nothing. */
#if NEWT_DEV_USB
size_t nsc_rx_capacity(void);
uint8_t nsc_connected(void);
int nsc_available(void);
int nsc_read_byte(void);
int nsc_available_for_write(void);
uint8_t nsc_wake_transmitter(void);
#endif
#ifdef __cplusplus
}
#endif
#endif
