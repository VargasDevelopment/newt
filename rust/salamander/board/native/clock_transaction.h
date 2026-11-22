/* SPDX-License-Identifier: MIT */
#ifndef NEWT_C3_CLOCK_TRANSACTION_H
#define NEWT_C3_CLOCK_TRANSACTION_H
#ifndef __cplusplus
#error "C++ only: callers pass an Arduino TwoWire-shaped bus"
#endif
#include "board.h"
#include <cstring>
/* The one DS3231 read behind nb_clock_read and nb_clock_snapshot, shared by
 * board.cpp (SDK Wire, ACTIVE address) and the host FakeWire check. Its bus
 * calls are firmware/home_clock.h readClockRegisters at bc9ffda5: the register
 * pointer is the only byte written, and the 10 ms timeout bounds the pointer
 * write and request before the caller's timeout is restored. A null pointer or
 * wrong length touches nothing; otherwise out is zeroed before any bus call. A
 * failed read keeps the bytes it initialized only when keep_prefix is set (for
 * Home's DEV line), else zeroes them again. No validity or clock policy. */
template <class Bus>
int32_t nb_clock_registers(Bus &bus, uint8_t address, uint8_t *out, size_t len,
                           bool keep_prefix) {
    if (!out) return NB_NULL;
    if (len != 16) return NB_LENGTH;
    std::memset(out, 0, len);
    if (!address) return NB_IO;
    const auto timeout = bus.getTimeOut();
    bus.setTimeOut(10);
    bus.beginTransmission(address);
    const bool pointer = bus.write(uint8_t{0}) == 1;
    const auto sent = bus.endTransmission(true);
    bool read = pointer && sent == 0 &&
        bus.requestFrom(address, uint8_t{16}, uint8_t{true}) == 16;
    if (read) for (size_t i = 0; i < len; ++i) {
        const int value = bus.read();
        if (value < 0) { read = false; break; }
        out[i] = uint8_t(value);
    }
    bus.setTimeOut(timeout);
    if (!read && !keep_prefix) std::memset(out, 0, len);
    return read ? NB_OK : NB_IO;
}
#endif
