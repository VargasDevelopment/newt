/* SPDX-License-Identifier: MIT */
#include "native_alloc.h"
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#if defined(ESP_PLATFORM)
#include <sdkconfig.h>
#if !defined(CONFIG_IDF_TARGET_ESP32C3) || !CONFIG_IDF_TARGET_ESP32C3 || \
    !defined(CONFIG_FREERTOS_UNICORE) || !CONFIG_FREERTOS_UNICORE
#error "Reviewed target is single-core ESP32-C3 only"
#endif
#if defined(CONFIG_HEAP_ABORT_WHEN_ALLOCATION_FAILS) && CONFIG_HEAP_ABORT_WHEN_ALLOCATION_FAILS
#error "Fallible Rust allocation requires the existing non-aborting heap policy"
#endif
_Static_assert(sizeof(void *) == 4 && sizeof(size_t) == 4, "C3 ABI");
#elif !defined(NEWT_ALLOC_HOST_TEST)
#error "Use actual C3 SDK configuration, or the explicit host test build"
#endif

/* The firmware's aligned_storage.h documents only four-byte malloc alignment.
 * Keep this same conservative policy on host; no host max_align_t assumption. */
static int valid(size_t size, size_t alignment) {
    return size && alignment && !(alignment & (alignment - 1)) &&
           alignment - 1 <= (size_t)PTRDIFF_MAX &&
           size <= (size_t)PTRDIFF_MAX - (alignment - 1);
}

void *newt_rust_heap_alloc(size_t size, size_t alignment) {
    if (!valid(size, alignment)) return NULL;
    if (alignment <= 4) return malloc(size);
    /* One saved pointer plus at most alignment-1 bytes; memcpy permits an
     * unaligned header. This retains libc's heap selection and free policy. */
    const size_t padding = alignment - 1;
    if (size > (size_t)PTRDIFF_MAX - sizeof(void *) ||
        padding > (size_t)PTRDIFF_MAX - sizeof(void *) - size) return NULL;
    unsigned char *raw = malloc(size + sizeof(void *) + padding);
    if (!raw) return NULL;
    unsigned char *begin = raw + sizeof(void *);
    const size_t residue = (uintptr_t)begin & padding;
    unsigned char *payload = begin + (residue ? alignment - residue : 0);
    memcpy(payload - sizeof(raw), &raw, sizeof(raw));
    return payload;
}

void *newt_rust_heap_zeroed(size_t size, size_t alignment) {
    void *pointer = newt_rust_heap_alloc(size, alignment);
    if (pointer) memset(pointer, 0, size);
    return pointer;
}

void newt_rust_heap_free(void *pointer, size_t size, size_t alignment) {
    (void)size;
    if (alignment <= 4) { free(pointer); return; }
    unsigned char *raw;
    memcpy(&raw, (unsigned char *)pointer - sizeof(raw), sizeof(raw));
    free(raw);
}

void *newt_rust_heap_resize(void *pointer, size_t old_size,
                            size_t alignment, size_t new_size) {
    if (!valid(new_size, alignment)) return NULL;
    if (alignment <= 4) return realloc(pointer, new_size);
    /* Allocate first: failure preserves old ownership, bytes and alignment.
     * Native realloc does not promise to preserve extended alignment. */
    void *next = newt_rust_heap_alloc(new_size, alignment);
    if (!next) return NULL;
    memcpy(next, pointer, old_size < new_size ? old_size : new_size);
    newt_rust_heap_free(pointer, old_size, alignment);
    return next;
}
