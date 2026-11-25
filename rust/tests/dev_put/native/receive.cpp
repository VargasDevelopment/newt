// SPDX-License-Identifier: MIT
#include <Arduino.h>
#include <cstring>
#include "serial_console.h"
#include "hal/usb_serial_jtag_ll.h"
HWCDC Serial;

static void simulate_rx_isr(uint32_t rx_bit) {
    // Models hw_cdc_isr_handler's masked INT_ST check: while RX is masked,
    // no byte can move from the hardware FIFO into Serial's rx_queue.
    if ((fake_mask & rx_bit) && (fake_raw & rx_bit)) {
        fake_raw &= ~rx_bit;
        while (!fake_fifo.empty()) {
            Serial.queued.push_back(fake_fifo.front());
            fake_fifo.pop_front();
        }
    }
}

int main() {
    nsc_begin();
#if NEWT_DEV_USB
    constexpr uint32_t RX = USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT;
    constexpr uint32_t TX = USB_SERIAL_JTAG_INTR_SERIAL_IN_EMPTY;
    constexpr uint32_t RESET = USB_SERIAL_JTAG_INTR_BUS_RESET;
    fake_mask = RX | TX | RESET;
    fake_fifo = {'z'};
    assert(nsc_available() == 0 && nsc_read_byte() == -1);
    assert(fake_fifo.size() == 1 && fake_mask == (RX | TX | RESET));

    // Every ordinary bridge operation still uses SDK RX, including refused PUT.
    Serial.queued = {'A', 'B'}; fake_fifo = {'C', 'D', 'E', 'F'}; fake_raw = RX;
    uint8_t queue_empty = 0;
    const uint32_t previous = nsc_put_rx_begin();
    assert(previous == RX && fake_mask == (TX | RESET) && fake_critical == 0);
    assert(nsc_put_rx_read(nullptr, 4, &queue_empty) == 0);
    uint8_t canary[12];
    std::memset(canary, 0xCC, sizeof(canary));
    assert(nsc_put_rx_read(canary + 4, 0, &queue_empty) == 0);

    // 1-byte read drains only the first queued byte; queue stays non-empty and FIFO untouched.
    assert(nsc_put_rx_read(canary + 4, 1, &queue_empty) == 1);
    assert(canary[4] == 'A' && canary[3] == 0xCC && canary[5] == 0xCC);
    assert(queue_empty == 0 && fake_fifo.size() == 4);

    // Bounded bulk read drains remaining queued byte then FIFO strictly up to max_len=3.
    std::memset(canary, 0xCC, sizeof(canary));
    assert(nsc_put_rx_read(canary + 4, 3, &queue_empty) == 3);
    assert(canary[3] == 0xCC && canary[4] == 'B' && canary[5] == 'C' && canary[6] == 'D' && canary[7] == 0xCC);
    assert(queue_empty == 1 && Serial.queued.empty() && fake_fifo.size() == 2);

    // Ordering proof: once queue_empty == 1 under the RX mask, simulated ISR ticks
    // cannot push bytes into Serial.queued, and Serial.available() is never called again.
    const unsigned avail_after_drain = Serial.available_calls;
    fake_fifo.push_back('G');
    fake_fifo.push_back('H');
    fake_raw |= RX;
    simulate_rx_isr(RX);
    assert(Serial.queued.empty());
    uint8_t tail[8] = {};
    assert(nsc_put_rx_read(tail, sizeof(tail), &queue_empty) == 4);
    assert(std::memcmp(tail, "EFGH", 4) == 0);
    assert(Serial.available_calls == avail_after_drain);
    assert(nsc_put_rx_read(tail, sizeof(tail), &queue_empty) == 0); // A poll never waits for data.
    assert(Serial.available_calls == avail_after_drain);

    fake_mask &= ~TX; // The SDK TX ISR may change TX during the loan.
    fake_fifo = {'H', '\n'}; // Next HELLO bytes while DONE is finishing.
    fake_raw = RX;
    nsc_put_rx_end(previous);
    assert(fake_mask == (RX | RESET) && fake_raw == RX && fake_critical == 0);
    // The normal ISR sees the pending RX event after unmasking.
    simulate_rx_isr(RX);
    assert(fake_raw == 0);
    assert(nsc_read_byte() == 'H' && nsc_read_byte() == '\n');
    assert(nsc_read_byte() == -1);

    // A second lease starts with a fresh queue_empty=0 and drains newly queued bytes before FIFO.
    fake_fifo = {'P', 'Q'}; fake_raw = RX;
    simulate_rx_isr(RX);
    fake_fifo = {'R', 'S'}; fake_raw = RX;
    const uint32_t prev2 = nsc_put_rx_begin();
    uint8_t queue_empty2 = 0;
    uint8_t both[8] = {};
    assert(nsc_put_rx_read(both, sizeof(both), &queue_empty2) == 4);
    assert(std::memcmp(both, "PQRS", 4) == 0 && queue_empty2 == 1);
    nsc_put_rx_end(prev2);

    fake_mask = TX | RESET;
    auto disabled = nsc_put_rx_begin(); nsc_put_rx_end(disabled);
    assert(disabled == 0 && fake_mask == (TX | RESET));
#endif
}
