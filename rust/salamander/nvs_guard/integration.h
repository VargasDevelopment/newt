// SPDX-License-Identifier: MIT

#pragma once
#include <cstddef>
#include <cstdint>
struct NewtGuard;
struct NewtGuardRefusals { std::uint8_t opens, writes; };
extern "C" {
// Raw address of the const-initialized Rust owner. Only sdk_glue.cpp may borrow
// it, for one leaf inside guardCall. Never retain a Rust reference across SDK.
// This boundary requires pinned ESP32-C3, single-core FreeRTOS (see sdk_glue.cpp).
NewtGuard* newt_guard_owner();
std::uint8_t newt_guard_begin_init(NewtGuard*);
std::int32_t newt_guard_finish_init(NewtGuard*, std::int32_t);
std::uint8_t newt_guard_deny_write(NewtGuard*, std::uint8_t, std::uint8_t, std::uint8_t, std::size_t);
std::uint8_t newt_guard_deny_erase(NewtGuard*, std::uint8_t, std::uint8_t, std::uint8_t,
                                  std::size_t, std::size_t, std::size_t);
std::uint8_t newt_guard_deny_open(NewtGuard*, std::int32_t);
std::int32_t newt_guard_deny_format(NewtGuard*);
std::uint8_t newt_guard_ready(const NewtGuard*);
std::uint8_t newt_guard_issue(const NewtGuard*);
const char* newt_guard_problem(const NewtGuard*);
void newt_guard_fence(NewtGuard*, std::uint8_t);
std::uint8_t newt_guard_fenced(const NewtGuard*);
NewtGuardRefusals newt_guard_refusals(const NewtGuard*);
}
