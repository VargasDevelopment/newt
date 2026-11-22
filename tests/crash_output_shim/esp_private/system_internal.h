#pragma once
// The pinned IDF's reset reasons (esp_system.h) and esp_system's reset hint
// (esp_private/system_internal.h), as rust/firmware/native/crash_output.h uses them.
enum esp_reset_reason_t {
  ESP_RST_UNKNOWN=0, ESP_RST_POWERON=1, ESP_RST_SW=3, ESP_RST_PANIC=4, ESP_RST_INT_WDT=5, ESP_RST_TASK_WDT=6,
  ESP_RST_DEEPSLEEP=8
};
extern "C" void esp_reset_reason_set_hint(esp_reset_reason_t hint);
extern "C" esp_reset_reason_t esp_reset_reason_get_hint(void);
