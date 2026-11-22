/* SPDX-License-Identifier: MIT */
#ifndef NEWT_C3_SERIAL_BRIDGE_H
#define NEWT_C3_SERIAL_BRIDGE_H
#ifndef __cplusplus
#error "C++ only: callers pass an Arduino Print-shaped port"
#endif
#include "serial_console.h"
/* The calls behind nsc_*, shared by serial_console.cpp (the SDK's Serial) and
 * the host FakeSerial check, as clock_transaction.h is for the RTC. The baud
 * is main.cpp's; HWCDC ignores it. */
template <class Port> void nsc_port_begin(Port &port) { port.begin(115200); }
template <class Port> void nsc_port_tx_timeout(Port &port, uint32_t ms) {
    port.setTxTimeoutMs(ms);
}
template <class Port>
size_t nsc_port_write(Port &port, const uint8_t *data, size_t len) {
    if (!data || !len) return 0;
    return port.write(data, len);
}
template <class Port> uint8_t nsc_port_connected(Port &port) {
    return static_cast<bool>(port);
}
template <class Port> int nsc_port_available(Port &port) { return port.available(); }
template <class Port> int nsc_port_read_byte(Port &port) { return port.read(); }
template <class Port> int nsc_port_available_for_write(Port &port) {
    return port.availableForWrite();
}
#endif
