#pragma once
// The pinned IDF's panic types and printer (esp_system's
// esp_private/panic_internal.h), as rust/firmware/native/crash_output.h uses them.
typedef void (*panic_info_dump_fn_t)(const void* frame);
typedef enum {
  PANIC_EXCEPTION_DEBUG, PANIC_EXCEPTION_IWDT, PANIC_EXCEPTION_TWDT, PANIC_EXCEPTION_ABORT, PANIC_EXCEPTION_FAULT
} panic_exception_t;
typedef struct {
  int core;
  panic_exception_t exception;
  const char* reason;
  const char* description;
  panic_info_dump_fn_t details;
  panic_info_dump_fn_t state;
  const void* addr;
  const void* frame;
  bool pseudo_excause;
} panic_info_t;
extern "C" void panic_print_str(const char* str);
