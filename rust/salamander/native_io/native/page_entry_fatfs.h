#pragma once
// The card's cache folder reader; sd_library.cpp is its one includer.
#include "page_entry_directory.h"
#include "sd_card_paths.h"
#include <cstdio>
#include <cstring>
#include "ff.h"

namespace newt::firmware {
// FatFs entries carry size and time, so a sized pass costs one folder read
// instead of a path lookup per name. fname is the whole long name, or the
// short name when the long name is missing or does not fit (never a cut long
// name): the same name the VFS readdir reports. Only the census calls this,
// inside its idle bus scope; FatFs's volume lock orders it with VFS calls.
// SD.begin mounts the card as drive 0, Newt's only FAT volume; any other path
// fails and the census denies.
bool sd_cache_detail::readPageEntries(const char* folder,bool,PageEntryVisit visit,void* context) {
  constexpr auto kMount=sizeof kSdMountPoint-1;
  if (std::strncmp(folder,kSdMountPoint,kMount) || folder[kMount]!='/') return false;
  char path[192];
  const auto bytes=std::snprintf(path,sizeof path,"0:%s",folder+kMount);
  FF_DIR directory{};
  if (bytes<=0 || static_cast<std::size_t>(bytes)>=sizeof path || f_opendir(&directory,path)!=FR_OK) return false;
  FILINFO info{};
  bool good=true;
  for (bool end=false;good && !end;) {
    good=f_readdir(&directory,&info)==FR_OK;
    end=!info.fname[0];
    good=good && visit(context,{end ? nullptr : info.fname,info.fsize,
        static_cast<std::int64_t>(info.fdate)<<16 | info.ftime,!(info.fattrib & AM_DIR)});
  }
  return f_closedir(&directory)==FR_OK && good;
}
} // namespace newt::firmware
