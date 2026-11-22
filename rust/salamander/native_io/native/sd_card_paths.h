#pragma once
// The card namespace Newt owns, defined once for the library scan, the
// prepared covers, the automatic cover producer and update mode.
// tests/update_card_test.py points the update directory at a scratch card.
#ifndef NEWT_SD_UPDATE_DIRECTORY
#define NEWT_SD_UPDATE_DIRECTORY "/newt-sd/Newt/Update"
#endif
namespace newt::firmware {
inline constexpr char kSdMountPoint[]="/newt-sd";
inline constexpr char kSdBooksDirectory[]="/newt-sd/Newt/Books";
inline constexpr char kSdCoversDirectory[]="/newt-sd/Newt/Covers";
// Update files (docs/083-ota-sd-install.md): <kind>.bin beside <kind>.manifest
// (firmware/update_card.h).
inline constexpr char kSdUpdateDirectory[]=NEWT_SD_UPDATE_DIRECTORY;
}  // namespace newt::firmware
