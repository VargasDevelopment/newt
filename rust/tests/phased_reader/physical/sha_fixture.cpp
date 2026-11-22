// SPDX-License-Identifier: MIT
// Use the existing inline host_sha256.h with Root's approved provider flags.
#include "newt/host_sha256.h"
extern "C" bool phase_sha256(const unsigned char* bytes, unsigned long len,
                             unsigned char* output) {
  return newt::host::sha256(bytes, len, output);
}
