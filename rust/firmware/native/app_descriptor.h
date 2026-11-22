#pragma once
#include "firmware_version.h"
#include <cstddef>
#include <cstdint>
#include <esp_app_desc.h>
#include <esp_ota_ops.h>
#include <esp_partition.h>
#include <mbedtls/sha256.h>

// Arduino confirms a pending image before setup() unless this returns true
// (esp32-hal-misc.c). Newt confirms after its first finished screen
// instead, so a build that cannot show one returns to the previous one on the
// next boot (docs/UPDATES.md, "Card install"). The Rust image supplies
// verifyRollbackLater and confirms its first finished paint.

namespace newt {
// The X3 app-slot identity block, the same rule as tools/x3_slots.py
// (docs/UPDATES.md, "Slots"). A slot's first kSlotHeadBytes hold the ESP
// image header, the first segment header, the ESP-IDF app descriptor and, for
// Newt, its fixed mark.
inline constexpr std::size_t kSlotHeadBytes = 0x200;
inline constexpr std::size_t kAppDescriptorAt = 0x20;
inline constexpr std::size_t kNewtMarkAt = kAppDescriptorAt + 0x100;
inline constexpr std::uint32_t kAppDescriptorMagic = 0xabcd5432u;
// Descriptor fields, as offsets into the 256-byte esp_app_desc_t.
inline constexpr std::size_t kDescriptorVersionAt = 16, kDescriptorProjectAt = 48, kDescriptorTextBytes = 32;

// Placed in .rodata_custom_desc, which the linker puts right after the app
// descriptor, so every Newt image carries it at kNewtMarkAt.
struct NewtAppMark {
  char magic[12];
  std::uint32_t format;
};
static_assert(sizeof(NewtAppMark) == 16);
inline constexpr NewtAppMark kNewtAppMark{"NEWT-X3-APP", 1};

// Newt's identity block follows the mark in the same section, so the mark
// and every tool that reads it stay unchanged (docs/UPDATES.md, "Slots"):
// the firmware version and the exact 12-hex build ID, each NUL-padded. A
// dirty or unidentified build carries an empty build. Images older than the
// block carry other bytes here; nothing trusts them without a manifest.
inline constexpr std::size_t kNewtIdentityAt = kNewtMarkAt + sizeof(NewtAppMark);
struct NewtAppIdentity {
  char version[24];
  char build[16];
};
struct NewtImageDescriptor {
  NewtAppMark mark;
  NewtAppIdentity identity;
};
static_assert(sizeof(NewtAppIdentity) == 40 && sizeof(NewtImageDescriptor) == 56);
static_assert(kNewtIdentityAt + sizeof(NewtAppIdentity) <= kSlotHeadBytes);
inline constexpr std::size_t kNewtBuildIdLength = 12;
constexpr bool exactBuildId(const char* id) {
  if (!id) return false;
  for (std::size_t i = 0; i < kNewtBuildIdLength; ++i)
    if (!((id[i] >= '0' && id[i] <= '9') || (id[i] >= 'a' && id[i] <= 'f'))) return false;
  return id[kNewtBuildIdLength] == '\0';
}
constexpr std::size_t identityTextLength(const char* text) {
  std::size_t length = 0;
  while (text[length]) ++length;
  return length;
}
// The block for `version` (which must fit) and `build` (kept only if exact).
constexpr NewtAppIdentity newtAppIdentity(const char* version, const char* build) {
  NewtAppIdentity identity{};
  for (std::size_t i = 0; version[i] && i + 1 < sizeof identity.version; ++i) identity.version[i] = version[i];
  if (exactBuildId(build))
    for (std::size_t i = 0; i < kNewtBuildIdLength; ++i) identity.build[i] = build[i];
  return identity;
}
}  // namespace newt

namespace newt::firmware {
#ifdef NEWT_DEV_BUILD_ID
inline constexpr char kNewtImageBuild[]=NEWT_DEV_BUILD_ID;
#else
inline constexpr char kNewtImageBuild[]="";
#endif
static_assert(identityTextLength(NEWT_FIRMWARE_VERSION)<sizeof(NewtAppIdentity::version));
// Every Newt image carries this mark right after its app descriptor, where
// classifySlot and tools/x3_slots.py look for it, then its
// version and exact build (docs/UPDATES.md, "Slots").
[[gnu::used, gnu::retain, gnu::section(".rodata_custom_desc")]] const NewtImageDescriptor kNewtImageDescriptor{kNewtAppMark,newtAppIdentity(NEWT_FIRMWARE_VERSION,kNewtImageBuild)};
}  // namespace newt::firmware

// Newt's ESP-IDF app descriptor, in place of the framework's weak one
// (esp_app_format's esp_app_desc.c), so other firmwares' updaters and tools
// name this image "Newt" and its version, not the precompiled framework's
// arduino-lib-builder. The other fields keep the framework's
// values. Time and date stay empty so a clean build stays reproducible;
// esptool still fills app_elf_sha256 and the image's checksum and digest.
static_assert(sizeof NEWT_FIRMWARE_VERSION<=sizeof(esp_app_desc_t::version));
extern "C" [[gnu::used, gnu::retain, gnu::section(".rodata_desc")]] const esp_app_desc_t esp_app_desc{
    .magic_word=ESP_APP_DESC_MAGIC_WORD,
    .secure_version=0,
    .reserv1={},
    .version=NEWT_FIRMWARE_VERSION,
    .project_name="Newt",
    .time="",
    .date="",
    .idf_ver=IDF_VER,
    .app_elf_sha256={},
    .min_efuse_blk_rev_full=CONFIG_ESP_EFUSE_BLOCK_REV_MIN_FULL,
    .max_efuse_blk_rev_full=CONFIG_ESP_EFUSE_BLOCK_REV_MAX_FULL,
    .mmu_page_size=31-__builtin_clz(CONFIG_MMU_PAGE_SIZE),
    .reserv3={},
    .reserv2={},
};
