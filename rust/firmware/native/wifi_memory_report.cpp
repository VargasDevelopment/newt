// SPDX-License-Identifier: MIT
// SDK walker projection only; row selection, totals and Serial policy are Rust.
#if defined(NEWT_DEV_USB) && NEWT_DEV_USB
#include <cstddef>
#include <cstdint>
#include <esp_heap_caps.h>

namespace {
using Visit = void (*)(void*, std::uintptr_t, std::uintptr_t, std::uintptr_t,
                      std::size_t, std::uint8_t);
struct Projection {
  void* context;
  Visit visit;
};
}

extern "C" void newt_wifi_memory_report_walk(void* context, Visit visit) {
  Projection projection{context, visit};
  heap_caps_walk_all([](walker_heap_into_t info, walker_block_info_t block, void* out) {
    const auto& projection = *static_cast<Projection*>(out);
    projection.visit(projection.context,
        static_cast<std::uintptr_t>(info.start), static_cast<std::uintptr_t>(info.end),
        reinterpret_cast<std::uintptr_t>(block.ptr), block.size, static_cast<std::uint8_t>(block.used));
    return true;
  }, &projection);
}
#endif
