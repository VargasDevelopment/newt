// SPDX-License-Identifier: MIT
// The SDK side of serial_console.h. Built only inside the firmware's Arduino
// build with its USB flags.
#include "serial_console.h"
#include "serial_bridge.h"
#include <Arduino.h>
#include <type_traits>
#if !ARDUINO_USB_MODE || !ARDUINO_USB_CDC_ON_BOOT
#error "Newt's Serial is the C3 USB Serial/JTAG HWCDC (platformio-rust-app.ini build_flags)"
#endif
static_assert(std::is_same<std::remove_reference_t<decltype(Serial)>, HWCDC>::value,
              "Serial must be HWCDCSerial: its write holds one lock per call");
#if NEWT_DEV_USB
static size_t put_rx_bytes;
size_t nsc_rx_capacity(void) { return put_rx_bytes; }
#endif
void nsc_begin(void) {
#if NEWT_DEV_USB
    // Before begin installs the ISR: never replace a live receive queue.
    put_rx_bytes = Serial.setRxBufferSize(4352);
#endif
    nsc_port_begin(Serial);
}
void nsc_set_tx_timeout_ms(uint32_t ms) { nsc_port_tx_timeout(Serial, ms); }
size_t nsc_write(const uint8_t *data, size_t len) { return nsc_port_write(Serial, data, len); }
#if NEWT_DEV_USB
#if defined(__GNUC__) && !defined(__clang__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wvolatile"
#endif
#include "hal/usb_serial_jtag_ll.h"
#if defined(__GNUC__) && !defined(__clang__)
#pragma GCC diagnostic pop
#endif
uint8_t nsc_connected(void) { return nsc_port_connected(Serial); }
int nsc_available(void) { return nsc_port_available(Serial); }
int nsc_read_byte(void) { return nsc_port_read_byte(Serial); }
int nsc_available_for_write(void) { return nsc_port_available_for_write(Serial); }
// Nonblocking transmit-tail wake over USB_SERIAL_JTAG. Reports whether the
// hardware EP1 IN FIFO had room so callers can send a trailing zero-length
// packet only after a full 64-byte packet has left the FIFO.
uint8_t __attribute__((noinline)) nsc_wake_transmitter(void) {
    const uint8_t writable = usb_serial_jtag_ll_txfifo_writable() ? 1 : 0;
    usb_serial_jtag_ll_txfifo_flush();
    usb_serial_jtag_ll_ena_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_IN_EMPTY);
    return writable;
}
#endif

extern "C" void nsc_flush() { Serial.flush(); }
