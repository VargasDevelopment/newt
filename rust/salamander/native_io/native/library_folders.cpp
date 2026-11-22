// SPDX-License-Identifier: MIT
// Native facts from bc9ffda firmware/library_directory.cpp:32-51 only.
#include "native_io.h"
#include "sd_card_paths.h"
#include "ff.h"
#include <array>
#include <cstdio>
#include <cstring>

namespace newt {
// Folders below the root the card walk enters: core library_catalog::FOLDER_DEPTH.
inline constexpr unsigned kLibraryFolderDepth = 6;
}  // namespace newt

namespace {
struct Folders {
  std::array<FF_DIR, newt::kLibraryFolderDepth + 1> levels{};
  FILINFO info{};
};
}
extern "C" uint32_t newt_io_library_scope(uint32_t levels,
    newt_io_library_scope_callback callback, void* context) {
  if (levels != newt::kLibraryFolderDepth + 1 || !callback || !context) return 0;
  Folders folders;
  return callback(&folders, context);
}
extern "C" uint32_t newt_io_library_open(void* state, uint32_t level, const char* path) {
  if (!state || !path || level > newt::kLibraryFolderDepth) return 0;
  auto& folders = *static_cast<Folders*>(state);
  constexpr auto mount = sizeof newt::firmware::kSdMountPoint - 1;
  if (std::strncmp(path, newt::firmware::kSdMountPoint, mount)
      || (path[mount] && path[mount] != '/')) return 0;
  char drive[256 + 3];
  const auto bytes = std::snprintf(drive, sizeof drive, "0:%s", path[mount] ? path + mount : "/");
  return bytes > 0 && static_cast<std::size_t>(bytes) < sizeof drive
      && f_opendir(&folders.levels[level], drive) == FR_OK;
}
extern "C" uint32_t newt_io_library_next(void* state, uint32_t level,
    newt_io_library_entry* entry) {
  if (!state || !entry || level > newt::kLibraryFolderDepth) return 0;
  auto& folders = *static_cast<Folders*>(state);
  if (f_readdir(&folders.levels[level], &folders.info) != FR_OK) return 0;
  if (!folders.info.fname[0]) return 1;
  *entry = {reinterpret_cast<const uint8_t*>(folders.info.fname), std::strlen(folders.info.fname),
      static_cast<uint32_t>(folders.info.fsize), (folders.info.fattrib & AM_DIR) != 0,
      (folders.info.fattrib & (AM_HID | AM_SYS)) != 0};
  return 2;
}
extern "C" uint32_t newt_io_library_close(void* state, uint32_t level) {
  if (!state || level > newt::kLibraryFolderDepth) return 0;
  return f_closedir(&static_cast<Folders*>(state)->levels[level]) == FR_OK;
}
