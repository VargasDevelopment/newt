#pragma once
// The pinned IDF's VFS unregister (esp_vfs.h), as rust/firmware/native/crash_output.h
// uses it.
#include "esp_err.h"
extern "C" esp_err_t esp_vfs_unregister(const char* base_path);
