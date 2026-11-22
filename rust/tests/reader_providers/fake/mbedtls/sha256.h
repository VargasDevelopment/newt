// SPDX-License-Identifier: MIT
// HOST FAKE of the pinned X3 mbedTLS SHA-256 entry points. This is not SDK
// runtime: it only records forwarding, injects failures and keeps context
// identity for the reader provider policy proof. Digests come from the
// existing host provider (newt::host::Sha256 over locked OpenSSL).
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Same inline 116-byte shape as the pinned ESP32-C3 DMA/resume context. The
// fake keeps an identity in total[], so a byte move (Rust return by value)
// keeps the context valid. The model is never keyed by context address.
typedef struct mbedtls_sha256_context {
  uint32_t total[2];
  uint32_t state[8];
  unsigned char buffer[64];
  int first_block;
  int mode;
  int sha_state;
} mbedtls_sha256_context;

enum {
  NEWT_FAKE_SHA_INIT = 1,
  NEWT_FAKE_SHA_FREE = 2,
  NEWT_FAKE_SHA_STARTS = 3,
  NEWT_FAKE_SHA_UPDATE = 4,
  NEWT_FAKE_SHA_FINISH = 5,
  NEWT_FAKE_SHA_ONESHOT = 6
};

// One recorded call. context is 0 for one-shot calls and unknown contexts;
// flag is is224 for starts and one-shot calls.
typedef struct newt_fake_sha_event {
  uint32_t op;
  uint32_t context;
  uint64_t length;
  int32_t flag;
  int32_t result;
} newt_fake_sha_event;

void mbedtls_sha256_init(mbedtls_sha256_context* ctx);
void mbedtls_sha256_free(mbedtls_sha256_context* ctx);
int mbedtls_sha256_starts(mbedtls_sha256_context* ctx, int is224);
int mbedtls_sha256_update(mbedtls_sha256_context* ctx, const unsigned char* input, size_t ilen);
int mbedtls_sha256_finish(mbedtls_sha256_context* ctx, unsigned char* output);
int mbedtls_sha256(const unsigned char* input, size_t ilen, unsigned char* output, int is224);

// Arms one failure (nonzero code) for the next call of op. A failed finish or
// one-shot call writes fixed bytes into its output, as a partial SDK write may.
void newt_fake_sha_fail_next(uint32_t op, int32_t code);
// Pops the oldest recorded call into event; returns 1 when one was written.
int newt_fake_sha_take(newt_fake_sha_event* event);

#ifdef __cplusplus
}
static_assert(sizeof(mbedtls_sha256_context) == 116, "pinned context size");
static_assert(sizeof(newt_fake_sha_event) == 24, "Rust event mirror");
#endif
