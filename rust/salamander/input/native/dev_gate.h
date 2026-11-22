/* SPDX-License-Identifier: MIT */
#pragma once
#include <freertos/FreeRTOS.h>

// Synchronization backing only: both Rust tasks borrow this byte under this
// one mux, and Rust makes every takeover decision.
struct np_dev_gate {
    portMUX_TYPE lock = portMUX_INITIALIZER_UNLOCKED;
    bool pending = false;
};
inline bool *np_port_gate_enter(np_dev_gate& gate) {
    portENTER_CRITICAL(&gate.lock);
    return &gate.pending;
}
inline void np_port_gate_exit(np_dev_gate& gate) {
    portEXIT_CRITICAL(&gate.lock);
}
