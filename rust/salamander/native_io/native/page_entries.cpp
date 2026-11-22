// MIT. Only adapts the existing production walker's callback to a C boundary.
#include "native_io.h"
#include "page_entry_directory.h"
#if defined(ESP_PLATFORM)
#include "page_entry_fatfs.h"
#endif
#include <cstring>
namespace {
struct Visit { newt_io_page_visit callback; void* context; };
bool receive(void* context,const newt::firmware::sd_cache_detail::PageEntry& entry) {
  const auto& visit=*static_cast<const Visit*>(context);
  return visit.callback(visit.context,reinterpret_cast<const std::uint8_t*>(entry.name),
      entry.name ? std::strlen(entry.name) : 0,entry.bytes,entry.time,entry.regular ? 1u : 0u)==1;
}
}
extern "C" int32_t newt_io_walk_pages(const char* folder,uint32_t sized,
    newt_io_page_visit callback,void* context) {
  if (!folder || sized>1 || !callback || !context) return 0;
  Visit visit{callback,context};
  return newt::firmware::sd_cache_detail::readPageEntries(folder,sized!=0,receive,&visit) ? 1 : 0;
}
