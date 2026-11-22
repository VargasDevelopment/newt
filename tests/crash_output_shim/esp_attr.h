#pragma once
// The pinned IDF's IRAM placement (esp_attr.h), as rust/firmware/native/crash_output.h
// uses it: the linked fixtures map .iram1.* to .iram0.text like the firmware.
#ifdef __riscv
#define IRAM_ATTR __attribute__((section(".iram1.0")))
#else
#define IRAM_ATTR
#endif
