#include "newt/host_sha256.h"
#include <algorithm>
#include <array>
#include <cassert>
#include <cstdio>
#include <cstring>
#include <string>

#if defined(__linux__) || defined(NEWT_HOST_SHA256_TEST_OPENSSL)
#include <openssl/provider.h>
#endif

namespace {
using Digest = std::array<std::uint8_t, 32>;
Digest expected(const char* hex) {
  assert(std::strlen(hex) == 64);
  Digest value{};
  const auto nibble = [](char c) -> unsigned {
    if (c >= '0' && c <= '9') return static_cast<unsigned>(c - '0');
    assert(c >= 'a' && c <= 'f');
    return static_cast<unsigned>(c - 'a' + 10);
  };
  for (unsigned i = 0; i < value.size(); ++i)
    value[i] = static_cast<std::uint8_t>((nibble(hex[i * 2]) << 4) | nibble(hex[i * 2 + 1]));
  return value;
}
void vector(const std::string& input, const char* hex) {
  const auto want = expected(hex);
  Digest digest{};
  assert(newt::host::sha256(input.data(), input.size(), digest.data()));
  assert(digest == want);
  // Cross block boundaries through unequal chunks and explicit empty updates.
  newt::host::Sha256 stream;
  assert(stream.start());
  assert(stream.update(nullptr, 0));
  constexpr std::array<std::size_t, 7> chunks{1, 2, 7, 63, 64, 65, 4096};
  std::size_t at = 0, chunk = 0;
  while (at < input.size()) {
    const auto size = std::min(chunks[chunk++ % chunks.size()], input.size() - at);
    assert(stream.update(input.data() + at, size));
    at += size;
  }
  assert(stream.finish(digest.data()) && digest == want);
  const auto finished = digest;
  assert(!stream.finish(digest.data()) && digest == finished);
  assert(!stream.update("x", 1));
  // Reinitialization starts an independent digest, including after finalization.
  assert(stream.start() && stream.update(input.data(), input.size()));
  assert(stream.finish(digest.data()) && digest == want);
}
void failures() {
  Digest out{}; out.fill(0x5a);
  const auto original = out;
  assert(!newt::host::sha256(nullptr, 1, out.data()) && out == original);
  assert(!newt::host::sha256("abc", 3, nullptr));
  newt::host::Sha256 hash;
  assert(!hash.update("abc", 3));
  assert(!hash.finish(out.data()) && out == original);
  assert(hash.start() && hash.update("abc", 3));
  assert(!hash.update(nullptr, 1));
  assert(!hash.finish(out.data()) && out == original);
  assert(hash.start() && hash.update("abc", 3));
  assert(!hash.finish(nullptr));
  assert(!hash.finish(out.data()) && out == original);
  assert(hash.start() && hash.update("discarded", 9));
  hash.reset();
  assert(!hash.finish(out.data()) && out == original);
  assert(hash.start() && hash.update("discarded", 9));
  assert(hash.start() && hash.update("abc", 3));
  assert(hash.finish(out.data()) && out == expected(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
  assert(newt::host::sha256(nullptr, 0, out.data()) && out == expected(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
  // Repeated destruction of an unfinished context exercises provider cleanup.
  for (unsigned i = 0; i < 64; ++i) {
    newt::host::Sha256 unfinished;
    assert(unfinished.start() && unfinished.update("abc", 3));
  }
}
}  // namespace
int main(int argc, char** argv) {
#if defined(__linux__) || defined(NEWT_HOST_SHA256_TEST_OPENSSL)
  if (argc == 2 && !std::strcmp(argv[1], "missing-provider")) {
    // A fresh process loads the real builtin null provider before SHA use. This
    // disables implicit default-provider loading; no fake crypto API is used.
    auto* provider = OSSL_PROVIDER_load(nullptr, "null");
    assert(provider);
    Digest out{}; out.fill(0x5a);
    const auto original = out;
    newt::host::Sha256 hash;
    assert(!hash.start());
    assert(!hash.update("abc", 3));
    assert(!hash.finish(out.data()) && out == original);
    assert(!newt::host::sha256("abc", 3, out.data()) && out == original);
    assert(OSSL_PROVIDER_unload(provider) == 1);
    std::puts("PASS: real OpenSSL unavailable-provider failure preserves output");
    return 0;
  }
#else
  (void)argv;
#endif
  assert(argc == 1);
  vector("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
  vector("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  vector("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
         "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
  vector(std::string(8192, 'a'), "dd4e6730520932767ec0a9e33fe19c4ce24399d6eba4ff62f13013c9ed30ef87");
  vector(std::string(1000000, 'a'), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
  failures();
  std::puts("PASS: real host SHA-256 one-shot/streaming vectors, restart, abort, cleanup and unchanged failure output");
}
