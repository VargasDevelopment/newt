#pragma once
// Every boot clears the RAM that becomes heap before IDF registers it
// (docs/UPDATES.md, "Secrets in RAM"). A restart or a panic reset keeps RAM, so an
// update-mode boot's freed blocks, the Wi-Fi driver's copies of the password
// and its derived keys among them, would otherwise reach a later boot's
// allocations. IDF registers the heap in two steps, and each wrap clears what
// that step registers, just before it: heap_caps_init asks for the available
// regions first and registers all but the startup stack's; main_task, once
// the scheduler runs and that stack is dead, registers the rest. The regions
// are exactly what IDF registers, so the ROM data and static data it reserves
// stay untouched. tools/verify-next-boot.py checks the wiring in the linked
// image; tests/heap_clear_test.py runs it through the platformio-rust-app.ini wraps.
#include <heap_memory_layout.h>
#include <cstddef>
#include <cstring>

namespace newt::firmware {
// The regions that are (or are not) the startup stack's, cleared.
[[gnu::noinline]] inline void clearHeapRegions(const soc_memory_region_t* regions, std::size_t count, bool startupStack) {
  for (std::size_t i = 0; i < count; ++i)
    if (regions[i].startup_stack == startupStack)
      std::memset(reinterpret_cast<void*>(regions[i].start), 0, regions[i].size);
}
}  // namespace newt::firmware

extern "C" {
std::size_t __real_soc_get_available_memory_regions(soc_memory_region_t* regions);
void __real_heap_caps_enable_nonos_stack_heaps();
// heap_caps_init's only source of regions: it registers them on return.
std::size_t __wrap_soc_get_available_memory_regions(soc_memory_region_t* regions) {
  const auto count = __real_soc_get_available_memory_regions(regions);
  newt::firmware::clearHeapRegions(regions, count, false);
  return count;
}
// main_task's first call, on its own heap stack: the startup stack is dead.
void __wrap_heap_caps_enable_nonos_stack_heaps() {
  const auto most = soc_get_available_memory_region_max_count();
  auto* regions = static_cast<soc_memory_region_t*>(__builtin_alloca(most * sizeof(soc_memory_region_t)));
  newt::firmware::clearHeapRegions(regions, __real_soc_get_available_memory_regions(regions), true);
  __real_heap_caps_enable_nonos_stack_heaps();
}
}
