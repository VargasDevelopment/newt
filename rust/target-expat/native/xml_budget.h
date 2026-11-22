#pragma once
#include <cstddef>

namespace newt {
// Aggregate requested allocation bytes, including aligned ownership headers,
// across live SDK XML parsers. libc bookkeeping/fragmentation remains outside
// this ceiling. Firmware's reader admission must reserve additional headroom.
inline constexpr std::size_t kXmlBudgetBytes = 32 * 1024;
struct XmlBudgetUsage {
  std::size_t liveBytes;
  std::size_t peakBytes;
  std::size_t rejectedAllocations;
};
XmlBudgetUsage xmlBudgetUsage();
}  // namespace newt
