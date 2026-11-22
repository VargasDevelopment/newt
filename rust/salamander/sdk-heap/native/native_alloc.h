/* SPDX-License-Identifier: MIT */
#ifndef NEWT_NATIVE_ALLOC_H
#define NEWT_NATIVE_ALLOC_H
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Nonzero Rust Layout: power-of-two alignment, rounded size <= PTRDIFF_MAX.
 * Ownership is exclusive; release/resize require the original size/alignment.
 * No pointer may cross between this API and direct libc free/realloc.
 * Invalid allocation sizes return NULL. Invalid owned pointers are UB.
 * resize NULL/zero are NOT libc extensions: callers must supply live storage
 * and nonzero sizes. Failure leaves the original allocation intact. */
void *newt_rust_heap_alloc(size_t size, size_t alignment);
void *newt_rust_heap_zeroed(size_t size, size_t alignment);
void newt_rust_heap_free(void *pointer, size_t size, size_t alignment);
void *newt_rust_heap_resize(void *pointer, size_t old_size,
                            size_t alignment, size_t new_size);
#ifdef __cplusplus
}
#endif
#endif
