// SPDX-License-Identifier: MIT
// Compile with the actual X3 SDK flags before linking reader_hash.rs.
#include <mbedtls/sha256.h>
#include <soc/soc_caps.h>
#include <cstddef>
#include <type_traits>

#if !defined(CONFIG_IDF_TARGET_ESP32C3) || !CONFIG_IDF_TARGET_ESP32C3
#error "reader_hash is specific to the pinned X3 ESP32-C3 SDK"
#endif
#if !defined(MBEDTLS_SHA256_ALT) || SOC_SHA_SUPPORT_PARALLEL_ENG || !(SOC_SHA_SUPPORT_DMA || SOC_SHA_SUPPORT_RESUME)
#error "reader_hash requires the reviewed SHA-256 DMA/resume context"
#endif

static_assert(std::is_standard_layout_v<mbedtls_sha256_context>);
static_assert(sizeof(mbedtls_sha256_context)==116);
static_assert(alignof(mbedtls_sha256_context)==4);
static_assert(offsetof(mbedtls_sha256_context,total)==0);
static_assert(offsetof(mbedtls_sha256_context,state)==8);
static_assert(offsetof(mbedtls_sha256_context,buffer)==40);
static_assert(offsetof(mbedtls_sha256_context,first_block)==104);
static_assert(offsetof(mbedtls_sha256_context,mode)==108);
static_assert(offsetof(mbedtls_sha256_context,sha_state)==112);
static_assert(sizeof(esp_sha_type)==sizeof(int));
static_assert(sizeof(esp_sha256_state)==sizeof(int));
