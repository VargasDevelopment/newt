// SPDX-License-Identifier: MIT
#include "board.h"
#include "frame_check.h"
#include "clock_transaction.h"
#include <FreeInkDisplay.h>
#include <InputManager.h>
#include "../../input/native/x3_input_compile_contract.h"
#include <PowerManager.h>
#include <XteinkDetect.h>
#include <Wire.h>
#include <SPI.h>
#include <SD.h>
#include "sd_shared_bus.h"
#include "sd_card_folders.h"
#include "../../native_io/native/sd_card_paths.h"
#include <sys/stat.h>
#include <driver/gpio.h>
#include <esp_sleep.h>
#include <esp_system.h>
#if !FREEINK_DEVICE_X3 || FREEINK_DEVICE_X4 || !EINK_DISPLAY_SINGLE_BUFFER_MODE
#error "This boundary requires X3-only single-buffer firmware flags"
#endif
static_assert(BoardConfig::XTEINK_X3.inputStyle == BoardConfig::InputStyle::XteinkAdcLadder);
static_assert(BoardConfig::XTEINK_X3_UC8279.inputStyle == BoardConfig::InputStyle::XteinkAdcLadder);
static_assert(freeink::FreeInkDisplay::X3_BUFFER_SIZE == 99 * 528);
static_assert(ESP_RST_POWERON == 1 && ESP_RST_SW == 3 && ESP_SLEEP_WAKEUP_GPIO == 7);
static_assert(InputManager::BTN_BACK == 0 && InputManager::BTN_CONFIRM == 1 &&
              InputManager::BTN_LEFT == 2 && InputManager::BTN_RIGHT == 3 &&
              InputManager::BTN_UP == 4 && InputManager::BTN_DOWN == 5 &&
              InputManager::BTN_POWER == 6);
using Display = freeink::FreeInkDisplay;
int32_t nb_select_x3(void *display) {
    if (!display) return NB_NULL;
    static_cast<Display*>(display)->setDisplayX3();
    return NB_OK;
}
void nb_release_sd(void) { BoardConfig::releaseSdRail(); }
int32_t nb_probe_panel(struct nb_probe *out) {
    if (!out) return NB_NULL;
    const bool promoted = freeink::applyXteinkDisplayController();
    const auto& d = freeink::getXteinkDisplayProbeDiag();
    *out = {{d.ver[0], d.ver[1], d.ver[2]}, d.verdict,
            uint8_t(d.valid), uint8_t(d.busyTimedOut), uint8_t(promoted)};
    return NB_OK; // Inconclusive is evidence, not a transport error.
}
// Same pinned SdLibrary::mount setup, then best-effort Update folder creation.
int32_t nb_mount_sd(void) {
    const auto& board = BoardConfig::ACTIVE;
    if (board.board != BoardConfig::Board::XteinkX3 &&
        board.board != BoardConfig::Board::XteinkX3Uc8279) return NB_MODE;
    struct Pins {
        void deselect(int pin) { pinMode(pin, OUTPUT); digitalWrite(pin, HIGH); }
        void releaseSdRail() { BoardConfig::releaseSdRail(); }
        void settle() { delay(10); }
    } pins;
    if (!newt::firmware::prepareSharedSdBus(SPI, pins, board)) return NB_IO;
    const uint32_t hz = board.sd.spiHz != 0 ? board.sd.spiHz : 40000000u;
    const bool mounted = SD.begin(board.sd.cs, SPI, hz, newt::firmware::kSdMountPoint, 3, false);
    return newt::firmware::prepareSdFolders(mounted,
        newt::firmware::kSdMountPoint, newt::firmware::kSdUpdateDirectory,
        [](const char* path) { return ::mkdir(path, 0700); })
        ? NB_OK : NB_IO;
}
int32_t nb_frame(void *display, struct nb_frame *out) {
    if (!out) return NB_NULL;
    *out = {}; // Failed calls never publish a stale pointer.
    if (!display) return NB_NULL;
    auto& d = *static_cast<Display*>(display);
    const struct nb_frame f{d.getFrameBuffer(), d.getBufferSize(),
        d.getDisplayWidth(), d.getDisplayHeight(), d.getDisplayWidthBytes()};
    const auto status = nb_check_frame(&f);
    if (status == NB_OK) *out = f;
    return status;
}
int32_t nb_lend_build(void *display, struct nb_storage *out) {
    if (!out) return NB_NULL;
    *out = {};
    if (!display) return NB_NULL;
    struct nb_frame before{};
    const auto status = nb_frame(display, &before);
    if (status != NB_OK) return status;
    auto& d = *static_cast<Display*>(display);
    uint32_t bytes = 0; // Match SDK declaration, not host unsigned-long width.
    auto* data = d.lendBuildStorage(&bytes);
    if (!data) return NB_IO; // SDK failed acquisition; nothing to return.
    if (data != before.data || bytes != before.len) {
        d.returnBuildStorage(); // A successful but invalid loan still needs return.
        return NB_FRAME;
    }
    *out = {data, size_t(bytes)};
    return NB_OK;
}
void nb_return_build(void *display) {
    if (display) static_cast<Display*>(display)->returnBuildStorage();
}
int32_t nb_begin(void *display) {
    if (!display) return NB_NULL;
    static_cast<Display*>(display)->begin(); // SDK fallible framebuffer allocation.
    struct nb_frame f{};
    return nb_frame(display, &f);
}
int32_t nb_update(void *display, uint32_t mode, uint32_t turn_off) {
    if (!display) return NB_NULL;
    if (mode > 2 || turn_off > 1) return NB_MODE;
    struct nb_frame f{};
    const auto status = nb_frame(display, &f);
    if (status != NB_OK) return status;
    const Display::RefreshMode modes[]{Display::FULL_REFRESH,
        Display::HALF_REFRESH, Display::FAST_REFRESH};
    static_cast<Display*>(display)->displayBuffer(modes[mode], turn_off != 0);
    return NB_OK;
}
int32_t nb_panel_sleep(void *display) {
    if (!display) return NB_NULL;
    static_cast<Display*>(display)->deepSleep();
    return NB_OK;
}
int32_t nb_input_begin(void *input) {
    if (!input) return NB_NULL;
    static_cast<InputManager*>(input)->begin();
    return NB_OK;
}
uint32_t nb_millis(void) { return millis(); }
uint32_t nb_micros(void) { return micros(); }
int32_t nb_clock_read(uint8_t *out, size_t len) {
    return nb_clock_registers(Wire, BoardConfig::ACTIVE.sensors.rtcAddr, out, len, false);
}
int32_t nb_clock_snapshot(uint8_t *out, size_t len) {
    return nb_clock_registers(Wire, BoardConfig::ACTIVE.sensors.rtcAddr, out, len, true);
}
extern "C" uint8_t nb_clock_write(uint8_t first, const uint8_t* bytes, size_t count) {
    const auto timeout = Wire.getTimeOut();
    Wire.setTimeOut(10);
    Wire.beginTransmission(BoardConfig::ACTIVE.sensors.rtcAddr);
    bool queued = Wire.write(first) == 1;
    for (size_t i = 0; i < count; ++i) queued = Wire.write(bytes[i]) == 1 && queued;
    const bool sent = Wire.endTransmission(true) == 0;
    Wire.setTimeOut(timeout);
    return queued && sent;
}
int32_t nb_boot(struct nb_boot *out) {
    if (!out) return NB_NULL;
    const auto reset = esp_reset_reason();
    *out = {uint32_t(reset), uint32_t(esp_sleep_get_wakeup_cause()),
            uint32_t(reset == ESP_RST_POWERON), uint32_t(reset == ESP_RST_DEEPSLEEP)};
    return NB_OK;
}
void nb_wait_power_release(void) { freeink::PowerManager::waitForPowerButtonRelease(); }
uint32_t nb_arm_power_wake(void) {
    return freeink::PowerManager::armPowerButtonWakeup();
}
void nb_power_down_rails(void) { freeink::PowerManager::powerDownRailsForSleep(); }
int32_t nb_try_sleep(void) {
    esp_sleep_config_gpio_isolate();
    gpio_deep_sleep_hold_en();
    return esp_deep_sleep_try_to_start();
}
void nb_restart(void) { esp_restart(); }

extern "C" void nb_delay_ms(uint32_t ms) { delay(ms); }
