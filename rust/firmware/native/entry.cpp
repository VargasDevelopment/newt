// SPDX-License-Identifier: MIT
// SDK object ownership only. Every application decision runs in Rust.
#include <Arduino.h>
#include <FreeInkDisplay.h>
#include <InputManager.h>
#include "heap_clear.h"
#include "crash_output.h"
#include "app_descriptor.h"
namespace {
freeink::FreeInkDisplay display(-1, -1, -1, -1, -1, -1);
InputManager input;
}
// Owner 2026-11-14: pay the calibration's stack gap from Rust's heap lead.
SET_LOOP_TASK_STACK_SIZE(20 * 1024)
extern "C" [[noreturn]] void newt_rust_app_run(void*, void*);
extern "C" void nb_allow_reading_crash_output() {
    newt::firmware::crashOutput = newt::firmware::kReadingBoot;
}
extern "C" void nb_withhold_console() { newt::firmware::withholdConsole(); }
void setup() { newt_rust_app_run(&display, &input); }
void loop() {}
