#pragma once
// What rust/firmware/native/heap_clear.h uses of IDF's heap_memory_layout.h, as IDF 5.5
// declares it.
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
typedef struct {
  intptr_t start;
  size_t size;
  size_t type;
  intptr_t iram_address;
  bool startup_stack;
} soc_memory_region_t;

size_t soc_get_available_memory_region_max_count(void);
size_t soc_get_available_memory_regions(soc_memory_region_t* regions);
#ifdef __cplusplus
}
#endif
