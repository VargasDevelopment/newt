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
void nsc_begin(void) { nsc_port_begin(Serial); }
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
// Same nonblocking transmit-tail wake as firmware/dev_usb_channel.h. No wait,
// retry, queue or app decision is hidden in this native hardware operation.
void __attribute__((noinline)) nsc_wake_transmitter(void) {
    usb_serial_jtag_ll_txfifo_flush();
    usb_serial_jtag_ll_ena_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_IN_EMPTY);
}
#endif

extern "C" void nsc_flush() { Serial.flush(); }
