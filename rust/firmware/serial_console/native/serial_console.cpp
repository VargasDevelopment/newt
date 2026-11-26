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
#if !CONFIG_IDF_TARGET_ESP32C3
#error "Scoped PUT receive requires the single-core ESP32-C3"
#endif
#if defined(__GNUC__) && !defined(__clang__)
#pragma GCC diagnostic pop
#endif
// The C3 has one core. Only these mask RMWs need a critical section;
// hw_cdc_isr_handler reads INT_ST (masked), so its RX branch cannot touch
// the FIFO while this loan owns RX. Its TX and reset branches stay enabled.
uint32_t nsc_put_rx_begin(void) {
    portMUX_TYPE lock = portMUX_INITIALIZER_UNLOCKED;
    portENTER_CRITICAL(&lock);
    uint32_t previous = usb_serial_jtag_ll_get_intr_ena_status() & USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT;
    usb_serial_jtag_ll_disable_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT);
    portEXIT_CRITICAL(&lock);
    return previous;
}
void nsc_put_rx_end(uint32_t previous) {
    portMUX_TYPE lock = portMUX_INITIALIZER_UNLOCKED;
    portENTER_CRITICAL(&lock);
    if (previous) usb_serial_jtag_ll_ena_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT);
    else usb_serial_jtag_ll_disable_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT);
    portEXIT_CRITICAL(&lock);
    // Do not clear RX raw status: the ISR must see any pending next request.
}
// Drains bytes queued before masking ahead of the hardware RX FIFO, writing at
// most max_len bytes. Caller must hold nsc_put_rx_begin() so hw_cdc_isr_handler
// cannot push into Serial's rx_queue once *queue_empty transitions to 1.
size_t nsc_put_rx_read(uint8_t *out, size_t max_len, uint8_t *queue_empty) {
    if (!out || !max_len) return 0;
    size_t got = 0;
    bool drained = queue_empty && *queue_empty;
    if (!drained) {
        int queued = nsc_port_available(Serial);
        while (got < max_len && queued > 0) {
            int byte = nsc_port_read_byte(Serial);
            if (byte < 0) break;
            out[got++] = static_cast<uint8_t>(byte);
            --queued;
        }
        if (queued <= 0) {
            drained = true;
            if (queue_empty) *queue_empty = 1;
        }
    }
    if (drained && got < max_len) {
        const size_t rem = max_len - got;
        const uint32_t want = rem > 4096u ? 4096u : static_cast<uint32_t>(rem);
        const int fifo = usb_serial_jtag_ll_read_rxfifo(out + got, want);
        if (fifo > 0) got += static_cast<size_t>(fifo);
    }
    return got;
}
uint8_t nsc_connected(void) { return nsc_port_connected(Serial); }
// The raw SOF monitor HWCDC::isPlugged reads, without HWCDC's latched flag.
uint8_t nsc_link_present(void) { return HWCDC::isPlugged() ? 1 : 0; }
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
