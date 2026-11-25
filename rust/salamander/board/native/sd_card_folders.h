// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstring>

namespace newt::firmware {
// Folder creation is best effort; its result never changes mount success.
template <std::size_t Size, class Mkdir>
bool prepareSdFolders(bool mounted, const char* mount, const char (&update)[Size], Mkdir mkdir) {
  if (!mounted) return false;
  char path[Size];
  std::memcpy(path, update, Size);
  const auto mount_bytes = std::strlen(mount);
  // Skip the VFS mount itself. An overridden fixture path may live elsewhere.
  const auto first = mount_bytes < Size - 1 && std::strncmp(path, mount, mount_bytes) == 0
      && path[mount_bytes] == '/' ? mount_bytes + 1 : 1;
  for (std::size_t end = first; end < Size; ++end) {
    if (path[end] != '/' && path[end] != '\0') continue;
    const char saved = path[end];
    path[end] = '\0';
    (void)mkdir(path);
    path[end] = saved;
  }
  return true;
}
}  // namespace newt::firmware
