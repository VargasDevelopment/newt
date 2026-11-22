#pragma once
// The pinned IDF's console putc (esp_rom_uart.h), as rust/firmware/native/crash_output.h
// uses it.
extern "C" void esp_rom_output_putc(char c);
