#pragma once
// The pinned IDF's esp_err.h, as rust/firmware/native/crash_output.h uses it.
typedef int esp_err_t;
#define ESP_OK 0
#define ESP_FAIL -1
