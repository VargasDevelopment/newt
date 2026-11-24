// SPDX-License-Identifier: MIT
#pragma once
// Extracted by the harness from the pinned ESP32-C3 HAL; no copied bit values.
#include "pinned-usb-irq.h"
inline uint32_t usb_serial_jtag_ll_get_intr_ena_status() { return fake_mask; }
inline void usb_serial_jtag_ll_disable_intr_mask(uint32_t bits) {
    assert(fake_critical == 1); fake_mask &= ~bits;
}
inline void usb_serial_jtag_ll_ena_intr_mask(uint32_t bits) {
    // Existing TX wake is outside the RX critical section.
    if (bits & USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT) assert(fake_critical == 1);
    fake_mask |= bits;
}
inline int usb_serial_jtag_ll_read_rxfifo(uint8_t* out, uint32_t n) {
    assert(!(fake_mask & USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT)); // Never race the SDK's RX ISR.
    uint32_t got = 0;
    while (got < n && !fake_fifo.empty()) {
        out[got++] = fake_fifo.front(); fake_fifo.pop_front();
    }
    return static_cast<int>(got);
}
inline void usb_serial_jtag_ll_txfifo_flush() {}
inline bool usb_serial_jtag_ll_txfifo_writable() { return true; }
