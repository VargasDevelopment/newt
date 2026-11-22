#pragma once
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <sys/stat.h>

#if !defined(ESP_PLATFORM)
#include <dirent.h>
// Host fault/ordering observations; production expands to POSIX.
#ifndef NEWT_PAGE_ENTRY_OPEN
#define NEWT_PAGE_ENTRY_OPEN ::opendir
#endif
#ifndef NEWT_PAGE_ENTRY_NEXT
inline int newtPageEntryNext(DIR* directory,struct dirent*,struct dirent** result) {
  errno=0; *result=::readdir(directory); return errno;
}
#define NEWT_PAGE_ENTRY_NEXT newtPageEntryNext
#endif
#ifndef NEWT_PAGE_ENTRY_CLOSE
#define NEWT_PAGE_ENTRY_CLOSE ::closedir
#endif
#endif
#ifndef NEWT_PAGE_ENTRY_STAT
// Acquired ESP FAT VFS has no symlinks and exposes stat, not lstat.
#if defined(ESP_PLATFORM)
#define NEWT_PAGE_ENTRY_STAT ::stat
#else
#define NEWT_PAGE_ENTRY_STAT ::lstat
#endif
#endif

namespace newt::firmware::sd_cache_detail {
// One cache folder entry; a null name ends the folder. Size, time and type
// are read only for a sized pass.
struct PageEntry { const char* name=nullptr; std::uint64_t bytes=0; std::int64_t time=0; bool regular=false; };
using PageEntryVisit=bool(*)(void*,const PageEntry&);
// Visits each entry except "." and "..", then the end; false on a read error or when a visit
// declines. The folder is closed either way and holds no regular-file slot.
// The card reads FatFs entries, which carry size and time (sd_library.cpp):
// the ESP FAT VFS dirent has neither, and a stat by path walks the folder
// from its first entry, so a stat per name costs O(n²) card reads.
#if defined(ESP_PLATFORM)
bool readPageEntries(const char* folder,bool sized,PageEntryVisit visit,void* context);
#else
inline bool readPageEntries(const char* folder,bool sized,PageEntryVisit visit,void* context) {
  auto* directory=NEWT_PAGE_ENTRY_OPEN(folder);
  if (!directory) return false;
  struct dirent storage{}; struct dirent* found=nullptr; char path[400];
  bool good=true;
  while (good) {
    good=!NEWT_PAGE_ENTRY_NEXT(directory,&storage,&found);
    if (good && found && (!std::strcmp(found->d_name,".") || !std::strcmp(found->d_name,".."))) continue;
    PageEntry entry{good && found ? found->d_name : nullptr};
    if (good && found && sized) {
      struct stat info{};
      const auto bytes=std::snprintf(path,sizeof path,"%s/%s",folder,found->d_name);
      good=bytes>0 && static_cast<std::size_t>(bytes)<sizeof path && !NEWT_PAGE_ENTRY_STAT(path,&info);
      entry.bytes=static_cast<std::uint64_t>(info.st_size); entry.time=info.st_mtime;
      entry.regular=S_ISREG(info.st_mode) && info.st_size>=0;
    }
    good=good && visit(context,entry);
    if (!found) break;
  }
  return !NEWT_PAGE_ENTRY_CLOSE(directory) && good;
}
#endif
} // namespace newt::firmware::sd_cache_detail
