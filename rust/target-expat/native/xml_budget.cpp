#include "xml_budget.h"
#include "aligned_storage.h"
#include <expat.h>
#include <algorithm>
#include <cstdlib>
#include <limits>

#ifdef NEWT_HOST_XML_HEAP_PROBE
extern "C" {
void* newt_xml_malloc(std::size_t) noexcept;
void* newt_xml_realloc(void*, std::size_t) noexcept;
void newt_xml_free(void*) noexcept;
}
#endif

namespace {
// The application owns one synchronous reader/parser flow. These counters are
// deliberately not a process-wide malloc replacement and are not thread-safe.
struct Header { void* raw; std::size_t bytes, payloadBytes; };
constexpr auto overhead=sizeof(Header)+alignof(std::max_align_t)-1;
static_assert(alignof(std::max_align_t)%alignof(Header)==0);
// Only payloads returned by allocate/resize reach this helper. They are aligned
// to max_align_t; subtracting sizeof(Header) preserves Header alignment. Express
// that producer guarantee to both approved compilers instead of increasing the
// alignment through a byte-pointer cast. Host UBSan checks the same assumption.
Header* allocationHeader(void* payload) {
  void* address=static_cast<std::uint8_t*>(payload)-sizeof(Header);
  return static_cast<Header*>(__builtin_assume_aligned(address,alignof(Header)));
}
newt::XmlBudgetUsage usage{};
bool admit(std::size_t retained, std::size_t requested) {
  if (requested > newt::kXmlBudgetBytes - retained) {
    ++usage.rejectedAllocations;
    return false;
  }
  return true;
}
void record(std::size_t bytes) {
  usage.liveBytes = bytes;
  if (bytes > usage.peakBytes) usage.peakBytes = bytes;
}
void* allocate(std::size_t size) {
  if (size > std::numeric_limits<std::size_t>::max() - overhead ||
      !admit(usage.liveBytes, size + overhead)) return nullptr;
#ifdef NEWT_HOST_XML_HEAP_PROBE
  auto* raw=newt_xml_malloc(size+overhead);
#else
  auto* raw=std::malloc(size+overhead);
#endif
  if (!raw) return nullptr;
  auto* payload=newt::alignedPayload(raw,sizeof(Header),alignof(std::max_align_t));
  auto* header=new(payload-sizeof(Header)) Header{raw,size+overhead,size};
  record(usage.liveBytes + header->bytes);
  return payload;
}
void release(void* pointer) {
  if (!pointer) return;
  auto* header=allocationHeader(pointer);
  usage.liveBytes -= header->bytes;
#ifdef NEWT_HOST_XML_HEAP_PROBE
  newt_xml_free(header->raw);
#else
  std::free(header->raw);
#endif
}
void* resize(void* pointer, std::size_t size) {
  if (!pointer) return allocate(size);
  if (!size) { release(pointer); return nullptr; }
  auto* old=allocationHeader(pointer);
  const auto retained = usage.liveBytes - old->bytes;
  if (size > std::numeric_limits<std::size_t>::max() - overhead ||
      !admit(retained, size + overhead)) return nullptr;
  const auto oldOffset=static_cast<std::uint8_t*>(pointer)-static_cast<std::uint8_t*>(old->raw);
  const auto copy=std::min(size,old->payloadBytes);
#ifdef NEWT_HOST_XML_HEAP_PROBE
  auto* raw=newt_xml_realloc(old->raw,size+overhead);
#else
  auto* raw=std::realloc(old->raw,size+overhead);
#endif
  // Failed realloc leaves both the old allocation and its accounting intact.
  if (!raw) return nullptr;
  auto* payload=newt::alignedPayload(raw,sizeof(Header),alignof(std::max_align_t));
  std::memmove(payload,static_cast<std::uint8_t*>(raw)+oldOffset,copy);
  auto* header=new(payload-sizeof(Header)) Header{raw,size+overhead,size};
  record(retained + header->bytes);
  return payload;
}
XML_Parser create(const XML_Char* encoding) {
  const XML_Memory_Handling_Suite memory{allocate, resize, release};
  return XML_ParserCreate_MM(encoding, &memory, nullptr);
}
}  // namespace

#ifdef NEWT_HOST_XML_HEAP_PROBE
// Host build renames only the vendored implementation's default constructor;
// the unchanged SDK call binds here. Firmware uses GNU ld --wrap instead.
extern "C" XML_Parser XMLCALL XML_ParserCreate(const XML_Char* encoding) {
  return create(encoding);
}
#else
extern "C" XML_Parser XMLCALL __wrap_XML_ParserCreate(const XML_Char* encoding) {
  return create(encoding);
}
#endif

namespace newt {
XmlBudgetUsage xmlBudgetUsage() { return usage; }
}  // namespace newt
