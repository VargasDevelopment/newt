// SPDX-License-Identifier: MIT
// Only the hardware/SDK leaves; the test compiles the actual native wrapper.
#pragma once
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <deque>
inline uint32_t fake_mask, fake_raw;
inline unsigned fake_critical;
inline std::deque<uint8_t> fake_fifo;
using portMUX_TYPE = int;
#define portMUX_INITIALIZER_UNLOCKED 0
#define portENTER_CRITICAL(lock) do { (void)(lock); ++fake_critical; } while (0)
#define portEXIT_CRITICAL(lock) do { (void)(lock); --fake_critical; } while (0)
struct HWCDC {
    std::deque<uint8_t> queued;
    unsigned available_calls = 0;
    void begin(unsigned long) {}
    void setTxTimeoutMs(uint32_t) {}
    size_t write(const uint8_t*, size_t n) { return n; }
    explicit operator bool() const { return true; }
    static bool isPlugged() { return true; }
    int available() { ++available_calls; return static_cast<int>(queued.size()); }
    int read() {
        if (queued.empty()) return -1;
        auto byte = queued.front(); queued.pop_front(); return byte;
    }
    int availableForWrite() { return 256; }
    void flush() {}
};
extern HWCDC Serial;
