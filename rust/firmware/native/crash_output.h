#pragma once
// Console output while secrets may be in RAM (docs/082-ota-updates.md, "Crash
// dumps and panic output"). One fail-closed word decides, and the gates sit
// on the console's channels rather than on each printer:
// - Panics and aborts: every one enters IDF's panic handler through
//   esp_panic_handler, which prints the reason or abort message, the fault's
//   details, registers and stack memory (or, with a debugger attached, an
//   address) and writes the core dump. The link wraps that entry
//   (platformio-rust-app.ini), so only a boot known to be a reading boot gets the real
//   handler. Any other boot prints one fixed line and restarts as the handler
//   would. esp_backtrace_print (the task watchdog's report) prints registers
//   outside the handler and meets the same check.
// - ROM print: esp_rom_printf, ets_printf and IDF's early logs (heap
//   poisoning's CORRUPT HEAP, ESP_ERROR_CHECK, the task watchdog's task list)
//   all end in the ROM's printf channels. Startup's first step installs an
//   IRAM gate as channel 1 and empties channel 2; the link turns every other
//   install into nothing.
// - stdout, stderr, esp_log and every VFS route to the console: update mode
//   sends the first three to /dev/null and unregisters the console's paths
//   before its first Wi-Fi call. Newt's DEV bridge and its own lines use the
//   USB serial driver, not these.
// tools/crash_output_guard.py proves from the linked image that nothing
// reaches the handler, its printers, the core dump writer, the ROM channels
// or the console's VFS tables except through these gates, and that only
// reviewed code builds or holds a console register address. Only main.cpp includes this (through
// firmware/update_mode.h), so each definition is single.
#include <esp_attr.h>
#include <esp_err.h>
#include <esp_log.h>
#include <esp_private/panic_internal.h>
#include <esp_private/startup_internal.h>
#include <esp_private/system_internal.h>
#include <esp_rom_uart.h>
#include <esp_vfs.h>
#include <cstdint>
#include <cstdio>

namespace newt::firmware {
// Fails closed: console output that could hold RAM is permitted only while the
// word holds exactly this value. It is .bss, so every boot starts at zero;
// setup writes it once it knows the boot is not update mode, and nothing
// clears it. Zero, any other value or a corrupted word withholds the output.
inline constexpr std::uint32_t kReadingBoot = 0x52454144u;  // "READ"
inline volatile std::uint32_t crashOutput = 0;

// Update mode's first step: IDF's and libraries' logs, printf and puts no
// longer reach the console in this boot, and neither does any descriptor.
// The console keeps its own descriptors on the UART and USB serial paths
// while stdin holds it open, and anything may open those paths, so each is
// unregistered: IDF drops every descriptor on it and later opens fail.
[[gnu::noinline]] inline void withholdConsole() {
  esp_log_level_set("*", ESP_LOG_NONE);
  std::freopen("/dev/null", "w", stdout);
  std::freopen("/dev/null", "w", stderr);
  esp_vfs_unregister("/dev/console");
  esp_vfs_unregister("/dev/uart");
  esp_vfs_unregister("/dev/secondary");
}
}  // namespace newt::firmware

extern "C" {
void __real_esp_panic_handler(panic_info_t*);
esp_err_t __real_esp_backtrace_print(int);
void __real_ets_install_putc1(void (*)(char));
void __real_ets_install_putc2(void (*)(char));
// panic.c's restart, the one esp_panic_handler ends with; no IDF header has it.
[[noreturn]] void panic_restart(void);

// platformio-rust-app.ini renames Arduino's wrap of the same name with a define.
#undef __wrap_esp_panic_handler
void __wrap_esp_panic_handler(panic_info_t* info) {
  if (newt::firmware::crashOutput == newt::firmware::kReadingBoot) return __real_esp_panic_handler(info);
  // No reason, message, address or number: an abort's message is caller RAM.
  panic_print_str("\r\nNewt: crash output withheld in this boot. Rebooting...\r\n");
  // The handler's own reset reason, so the next boot sees a panic or watchdog
  // rather than a software restart (which could honor an update request).
  if (esp_reset_reason_get_hint() == ESP_RST_UNKNOWN)
    esp_reset_reason_set_hint(info->exception == PANIC_EXCEPTION_IWDT   ? ESP_RST_INT_WDT
                              : info->exception == PANIC_EXCEPTION_TWDT ? ESP_RST_TASK_WDT
                                                                        : ESP_RST_PANIC);
  panic_restart();
}
esp_err_t __wrap_esp_backtrace_print(int depth) {
  if (newt::firmware::crashOutput == newt::firmware::kReadingBoot) return __real_esp_backtrace_print(depth);
  return ESP_FAIL;
}

// ROM print's only channel. IRAM, reading only a DRAM word, so it runs with the
// cache off (flash writes print). esp_rom_output_putc is IDF's equivalent of
// the putc the bootloader installs (esp_rom_install_uart_printf).
IRAM_ATTR void newt_console_putc(char c) {
  if (newt::firmware::crashOutput == newt::firmware::kReadingBoot) esp_rom_output_putc(c);
}
// Arduino's setDebugOutput and uartEnd would replace the gate.
void __wrap_ets_install_putc1(void (*)(char)) {}
void __wrap_ets_install_putc2(void (*)(char)) {}
}  // extern "C"

// Startup's first init step, before any IDF component's: from here on ROM
// print passes the gate. Priority 0 sorts ahead of IDF's lowest (1).
ESP_SYSTEM_INIT_FN(newt_console_gate, CORE, BIT(0), 0) {
  __real_ets_install_putc1(newt_console_putc);
  __real_ets_install_putc2(nullptr);
  return ESP_OK;
}
