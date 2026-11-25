#include "../rust/salamander/board/native/sd_shared_bus.h"
#include "../rust/salamander/board/native/sd_card_folders.h"
#include <array>
#include <cassert>
#include <cerrno>
#include <cstring>
#include <cstdio>

namespace {
struct Board {
  struct Display { int sclk = 8, mosi = 10, cs = 21; } display;
  struct Sd { int miso = 7, cs = 12, powerEnable = 13; } sd;
};
struct Pins {
  std::array<bool, 22> high{};
  bool settled = false;
  void deselect(int pin) { high[pin] = true; }
  void releaseSdRail() { assert(high[21] && high[12]); high[13] = true; }
  void settle() { assert(high[21] && high[12] && high[13]); settled = true; }
};
struct Spi {
  explicit Spi(const Pins& p) : pins(p) {}
  const Pins& pins;
  bool initialized = true, beginOk = true;
  int clock = 8, miso = -1, mosi = 10, cs = 21;
  unsigned ends = 0;
  void end() {
    assert(pins.high[21] && pins.high[12] && pins.high[13] && pins.settled);
    initialized = false;
    ++ends;
  }
  bool begin(int c, int i, int o, int s) {
    if (initialized) return true;  // Installed Arduino's display-first behavior.
    clock = c; miso = i; mosi = o; cs = s;
    initialized = beginOk;
    return beginOk;
  }
};
}  // namespace
int main() {
  Board board;
  Pins pins;
  Spi shared(pins);
  assert(shared.initialized && shared.miso == -1);
  assert(newt::firmware::prepareSharedSdBus(shared, pins, board));
  assert(shared.ends == 1 && shared.initialized && shared.clock == 8 && shared.mosi == 10 && shared.miso == 7 && shared.cs == 12);
  Pins otherPins;
  Spi failed(otherPins);
  failed.beginOk = false;
  assert(!newt::firmware::prepareSharedSdBus(failed, otherPins, board) && !failed.initialized);
  std::puts("PASS: idle shared bus deselects both devices, reattaches SD MISO after display-first initialization, and reports begin failure");
  // Fresh/existing folders, read-only cards and full cards all keep the mount.
  for (int root : {0, EEXIST, EACCES, ENOSPC}) {
    for (int update : {0, EEXIST, EACCES, ENOSPC}) {
      unsigned calls = 0;
      auto mkdir = [&](const char* path) {
        assert(calls < 2);
        assert(std::strcmp(path, calls == 0 ? "/newt-sd/Newt" : "/newt-sd/Newt/Update") == 0);
        return calls++ == 0 ? root : update;
      };
      assert(!newt::firmware::prepareSdFolders(false, mkdir) && calls == 0);
      assert(newt::firmware::prepareSdFolders(true, mkdir) && calls == 2);
    }
  }
  std::puts("PASS: Update folder creation follows a successful mount and never blocks a mounted card");
}
