// SPDX-License-Identifier: MIT
#pragma once
#include "../../native_io/native/sd_card_paths.h"

namespace newt::firmware {
// A folder failure must not turn a mounted card into a reading failure.
template <class Mkdir>
bool prepareSdFolders(bool mounted, Mkdir mkdir) {
  if (!mounted) return false;
  (void)mkdir("/newt-sd/Newt");
  (void)mkdir(kSdUpdateDirectory);
  return true;
}
}  // namespace newt::firmware
