// IDF's startup heap registration, in the objects IDF keeps it in, so the
// platformio-rust-app.ini wraps apply to the same undefined references they do on the
// device (tests/heap_clear_test.py). One file, one object per part.
#include <heap_memory_layout.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <initializer_list>

[[noreturn]] inline void fail(const char* expected) {
  std::fprintf(stderr, "FAIL heap clear: %s\n", expected);
  std::exit(1);
}

// RAM: the startup stack's region, two heap regions and data IDF reserves
// (ROM and static data, never listed) right after them.
enum { kStartupStack, kHeapA, kHeapB, kReserved, kRamBlocks };
inline constexpr std::size_t kBlockBytes = 64;
inline constexpr unsigned char kLeftover = 0xee;  // a panicked update-mode boot's bytes
inline constexpr unsigned char kLive = 0x5a;      // the startup stack's live frames
extern "C" unsigned char ram[kRamBlocks][kBlockBytes];

#if defined(HEAP_CLEAR_LAYOUT)  // memory_layout_utils.c
unsigned char ram[kRamBlocks][kBlockBytes];

extern "C" size_t soc_get_available_memory_region_max_count(void) { return kRamBlocks; }

extern "C" size_t soc_get_available_memory_regions(soc_memory_region_t* regions) {
  std::size_t count = 0;
  for (int block : {kHeapA, kHeapB, kStartupStack})
    regions[count++] = {reinterpret_cast<intptr_t>(ram[block]), kBlockBytes, 0, 0, block == kStartupStack};
  return count;
}

#elif defined(HEAP_CLEAR_HEAP)  // heap_caps_init.c: registered_heaps
namespace {
soc_memory_region_t later[kRamBlocks];
std::size_t laterCount = 0;

void registerHeap(const soc_memory_region_t& region, const char* expected) {
  const auto* bytes = reinterpret_cast<const unsigned char*>(region.start);
  for (std::size_t i = 0; i < region.size; ++i)
    if (bytes[i] != 0) fail(expected);
}
}  // namespace

// Registers every region but the startup stack's, and keeps those for later.
extern "C" void heap_caps_init(void) {
  soc_memory_region_t regions[kRamBlocks];
  const std::size_t count = soc_get_available_memory_regions(regions);
  for (std::size_t i = 0; i < count; ++i) {
    if (regions[i].startup_stack)
      later[laterCount++] = regions[i];
    else
      registerHeap(regions[i], "heap_caps_init registers only cleared RAM");
  }
}

// Registers the kept regions; it asks the layout for nothing.
extern "C" void heap_caps_enable_nonos_stack_heaps(void) {
  for (std::size_t i = 0; i < laterCount; ++i) registerHeap(later[i], "main_task registers only cleared RAM");
}

#elif defined(HEAP_CLEAR_STARTUP)  // the startup code and app_startup.c
extern "C" void heap_caps_init(void);
extern "C" void heap_caps_enable_nonos_stack_heaps(void);

namespace {
void expectUntouched(int block, unsigned char value, const char* expected) {
  for (unsigned char byte : ram[block])
    if (byte != value) fail(expected);
}
}  // namespace

int main() {
  std::memset(ram, kLeftover, sizeof ram);
  std::memset(ram[kStartupStack], kLive, kBlockBytes);
  heap_caps_init();
  expectUntouched(kStartupStack, kLive, "the live startup stack survives heap_caps_init");
  // The scheduler starts; main_task runs on a heap stack, the startup stack is dead.
  heap_caps_enable_nonos_stack_heaps();
  expectUntouched(kReserved, kLeftover, "ROM and static data stay untouched");
  std::puts("PASS heap clear: each startup step registers only cleared RAM");
}
#endif
