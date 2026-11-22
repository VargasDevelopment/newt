#pragma once
#include <cstdint>

namespace newt {
// Why shared NVS is not usable, as rust/salamander/nvs_guard reports it.
enum class NvsIssue : std::uint8_t { None, Version, Recovery, Unavailable };
}  // namespace newt

namespace newt::firmware {
// Arduino initializes shared NVS before setup(). Unknown/failed initialization
// cannot authorize Newt stores to open or replace somebody else's records.
bool nvsStorageReady();
NvsIssue nvsStorageIssue();
const char* nvsStorageProblem();  // nullptr when usable; safe UI text otherwise
// Wi-Fi never writes flash (docs/084-wifi-memory.md). While the fence is up,
// every partition write or erase and every read-write NVS open is refused,
// so the PHY's calibration store fails at its open and the radio keeps this
// boot's calibration. Raise it before Wi-Fi starts; lower it only once Wi-Fi
// is deinitialized again. Raising it starts a new count of what it refused,
// for update mode's DEV row.
void fenceFlashForWifi(bool raised);
// Raised: Wi-Fi may run, so update mode commits no saved network (docs/085)
// and the system bar says so.
bool flashFencedForWifi();
// Read-write opens; partition writes and erases; 255 means 255 or more.
struct FlashFenceRefusals { std::uint8_t opens = 0, writes = 0; };
FlashFenceRefusals flashFenceRefusals();
}  // namespace newt::firmware
