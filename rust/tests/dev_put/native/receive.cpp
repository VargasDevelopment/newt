// SPDX-License-Identifier: MIT
#include <Arduino.h>
#include "serial_console.h"
HWCDC Serial;
int main() {
    nsc_begin();
#if NEWT_DEV_USB
    constexpr uint32_t RX = 4u, TX = 2u, RESET = 8u;
    fake_mask = RX | TX | RESET;
    fake_fifo = {'z'};
    assert(nsc_available() == 0 && nsc_read_byte() == -1);
    assert(fake_fifo.size() == 1 && fake_mask == (RX | TX | RESET));
    // Every ordinary bridge operation still uses SDK RX, including refused PUT.
    Serial.queued = {'A', 'B'}; fake_fifo = {'C', 'D'}; fake_raw = RX;
    const uint32_t previous = nsc_put_rx_begin();
    assert(previous == RX && fake_mask == (TX | RESET) && fake_critical == 0);
    for (int expected : {'A', 'B', 'C', 'D'}) assert(nsc_put_rx_read_byte() == expected);
    assert(nsc_put_rx_read_byte() == -1); // A poll never waits for data.
    fake_mask &= ~TX; // The SDK TX ISR may change TX during the loan.
    fake_fifo = {'H', '\n'}; // Next HELLO bytes while DONE is finishing.
    nsc_put_rx_end(previous);
    assert(fake_mask == (RX | RESET) && fake_raw == RX && fake_critical == 0);
    // The normal ISR sees the pending RX event after unmasking.
    while (!fake_fifo.empty()) { Serial.queued.push_back(fake_fifo.front()); fake_fifo.pop_front(); }
    fake_raw = 0;
    assert(nsc_read_byte() == 'H' && nsc_read_byte() == '\n');
    assert(nsc_read_byte() == -1);
    fake_mask = TX | RESET;
    auto disabled = nsc_put_rx_begin(); nsc_put_rx_end(disabled);
    assert(disabled == 0 && fake_mask == (TX | RESET));
#endif
}
