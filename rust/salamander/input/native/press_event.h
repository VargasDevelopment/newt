/* SPDX-License-Identifier: MIT */
#pragma once
#include <cstdint>
#ifndef NEWT_DEV_USB
#define NEWT_DEV_USB 0
#endif
#if NEWT_DEV_USB
namespace newt::dev {
// One queued press: its DEV USB ticket (0 for a physical press) and button.
struct ButtonEvent { std::uint32_t ticket; std::uint8_t button; };
static_assert(sizeof(ButtonEvent) == 8);
}  // namespace newt::dev
#endif
