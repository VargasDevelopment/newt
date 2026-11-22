#pragma once
// Host-only provider adapter. Firmware continues to use its pinned mbedTLS API.
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>

#if defined(__linux__) || defined(NEWT_HOST_SHA256_TEST_OPENSSL)
#include <openssl/evp.h>
#elif defined(__APPLE__)
#include <CommonCrypto/CommonDigest.h>
#else
#error "Newt host SHA-256 requires the reviewed CommonCrypto or OpenSSL provider"
#endif

namespace newt::host {
// Finalization publishes exactly 32 bytes only after a successful provider call.
// A failed/aborted operation cannot later finish as a successful partial digest.
class Sha256 {
 public:
  Sha256() = default;
  Sha256(const Sha256&) = delete;
  Sha256& operator=(const Sha256&) = delete;
  ~Sha256() { reset(); }
  void reset() {
#if defined(__linux__) || defined(NEWT_HOST_SHA256_TEST_OPENSSL)
    EVP_MD_CTX_free(context_);
    context_ = nullptr;
#endif
    active_ = false;
  }
  bool start() {
    reset();
#if defined(__linux__) || defined(NEWT_HOST_SHA256_TEST_OPENSSL)
    // SHA uses the reviewed builtin provider; ambient configuration must not
    // replace it or add external modules before the first EVP operation.
    if (OPENSSL_init_crypto(OPENSSL_INIT_NO_LOAD_CONFIG, nullptr) != 1) return false;
    context_ = EVP_MD_CTX_new();
    if (!context_ || EVP_DigestInit_ex(context_, EVP_sha256(), nullptr) != 1) {
      reset();
      return false;
    }
#else
    if (CC_SHA256_Init(&context_) != 1) return false;
#endif
    active_ = true;
    return true;
  }
  bool update(const void* bytes, std::size_t size) {
    if (!active_) return false;
    if (!bytes && size) { reset(); return false; }
    // Empty chunks are valid, including a null pointer with zero length.
    if (!size) return true;
#if defined(__linux__) || defined(NEWT_HOST_SHA256_TEST_OPENSSL)
    const bool okay = EVP_DigestUpdate(context_, bytes, size) == 1;
#else
    if (size > std::numeric_limits<CC_LONG>::max()) { reset(); return false; }
    const bool okay = CC_SHA256_Update(&context_, bytes, static_cast<CC_LONG>(size)) == 1;
#endif
    if (!okay) reset();
    return okay;
  }
  bool finish(std::uint8_t* out) {
    if (!active_) return false;
    if (!out) { reset(); return false; }
#if defined(__linux__) || defined(NEWT_HOST_SHA256_TEST_OPENSSL)
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned size = 0;
    const bool okay = EVP_DigestFinal_ex(context_, digest.data(), &size) == 1 && size == 32;
#else
    std::array<unsigned char, CC_SHA256_DIGEST_LENGTH> digest{};
    static_assert(CC_SHA256_DIGEST_LENGTH == 32);
    const bool okay = CC_SHA256_Final(digest.data(), &context_) == 1;
#endif
    reset();
    if (!okay) return false;
    std::memcpy(out, digest.data(), 32);
    return true;
  }
 private:
#if defined(__linux__) || defined(NEWT_HOST_SHA256_TEST_OPENSSL)
  EVP_MD_CTX* context_ = nullptr;
#else
  CC_SHA256_CTX context_{};
#endif
  bool active_ = false;
};

inline bool sha256(const void* bytes, std::size_t size, std::uint8_t* out) {
  if ((!bytes && size) || !out) return false;
  Sha256 hash;
  return hash.start() && hash.update(bytes, size) && hash.finish(out);
}
}  // namespace newt::host
