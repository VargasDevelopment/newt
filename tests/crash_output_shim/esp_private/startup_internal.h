#pragma once
// The pinned IDF's startup init steps (esp_private/startup_internal.h), as
// rust/firmware/native/crash_output.h uses them. The linked RISC-V fixture sorts them by
// priority like the firmware's linker script; a host build collects them in
// one named section that the harness walks (__start_/__stop_).
#include <cstdint>
#include "esp_err.h"
#ifndef BIT
#define BIT(nr) (1UL << (nr))
#endif
typedef struct {
  esp_err_t (*fn)(void);
  uint16_t cores;
  uint16_t stage;
} esp_system_init_fn_t;
#define ESP_SYSTEM_INIT_STAGE_CORE 0
#define ESP_SYSTEM_INIT_STAGE_SECONDARY 1
#ifdef __riscv
#define NEWT_INIT_FN_SECTION(priority) ".esp_system_init_fn." #priority
#else
#define NEWT_INIT_FN_SECTION(priority) "newt_host_init_fn"
#endif
// IDF's trailing attribute arguments are left out; crash_output.h passes none.
#define ESP_SYSTEM_INIT_FN(f, stage_, c, priority) \
  static esp_err_t __esp_system_init_fn_##f(void); \
  static __attribute__((used, section(NEWT_INIT_FN_SECTION(priority)))) esp_system_init_fn_t \
      esp_system_init_fn_##f = {__esp_system_init_fn_##f, (c), ESP_SYSTEM_INIT_STAGE_##stage_}; \
  static esp_err_t __esp_system_init_fn_##f(void)
