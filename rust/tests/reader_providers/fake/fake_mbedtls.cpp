// SPDX-License-Identifier: MIT
// Host fake mbedTLS SHA-256; see mbedtls/sha256.h. One object serves both the
// frozen C++ oracle and the Rust provider driver.
#include "mbedtls/sha256.h"
#include "newt/host_sha256.h"
#include <cstring>
#include <deque>
#include <map>

namespace {
constexpr std::uint32_t kLive = 0x4E465348;
// Fake's own refusal (SHA-224, unknown or unstarted context), mbedTLS-negative.
constexpr int kRefused = -0x0074;

struct Model {
  std::map<std::uint32_t, newt::host::Sha256> contexts;
  std::deque<newt_fake_sha_event> events;
  std::uint32_t next = 1, failOp = 0;
  std::int32_t failCode = 0;
};

// Never destroyed: a context left unfreed by a control stays reachable, and no
// OpenSSL object is released after OpenSSL's own exit cleanup.
Model& model() {
  static Model* value = new Model;
  return *value;
}

std::uint32_t identity(const mbedtls_sha256_context* ctx) {
  return ctx && ctx->total[1] == kLive ? ctx->total[0] : 0;
}

newt::host::Sha256* backing(const mbedtls_sha256_context* ctx) {
  const auto found = model().contexts.find(identity(ctx));
  return found == model().contexts.end() ? nullptr : &found->second;
}

std::int32_t injected(std::uint32_t op) {
  auto& state = model();
  if (state.failOp != op) return 0;
  state.failOp = 0;
  return state.failCode;
}

void record(std::uint32_t op, std::uint32_t context, std::uint64_t length,
            std::int32_t flag, std::int32_t result) {
  model().events.push_back({op, context, length, flag, result});
}

void scribble(unsigned char* output) {
  for (unsigned i = 0; i < 32; ++i) output[i] = static_cast<unsigned char>(0xE0 | (i & 15));
}
}  // namespace

extern "C" {
void mbedtls_sha256_init(mbedtls_sha256_context* ctx) {
  std::memset(ctx, 0, sizeof *ctx);
  const auto id = model().next++;
  model().contexts.try_emplace(id);
  ctx->total[0] = id;
  ctx->total[1] = kLive;
  record(NEWT_FAKE_SHA_INIT, id, 0, 0, 0);
}

void mbedtls_sha256_free(mbedtls_sha256_context* ctx) {
  const auto id = identity(ctx);
  if (id) model().contexts.erase(id);
  if (ctx) std::memset(ctx, 0, sizeof *ctx);
  record(NEWT_FAKE_SHA_FREE, id, 0, 0, 0);
}

int mbedtls_sha256_starts(mbedtls_sha256_context* ctx, int is224) {
  auto* sha = backing(ctx);
  std::int32_t result = injected(NEWT_FAKE_SHA_STARTS);
  if (!result && (!sha || is224 || !sha->start())) result = kRefused;
  record(NEWT_FAKE_SHA_STARTS, identity(ctx), 0, is224, result);
  return result;
}

int mbedtls_sha256_update(mbedtls_sha256_context* ctx, const unsigned char* input, size_t ilen) {
  auto* sha = backing(ctx);
  std::int32_t result = injected(NEWT_FAKE_SHA_UPDATE);
  // An injected failure consumes no input.
  if (!result && (!sha || !sha->update(input, ilen))) result = kRefused;
  record(NEWT_FAKE_SHA_UPDATE, identity(ctx), ilen, 0, result);
  return result;
}

int mbedtls_sha256_finish(mbedtls_sha256_context* ctx, unsigned char* output) {
  auto* sha = backing(ctx);
  std::int32_t result = injected(NEWT_FAKE_SHA_FINISH);
  if (result) scribble(output);
  else if (!sha || !sha->finish(output)) result = kRefused;
  record(NEWT_FAKE_SHA_FINISH, identity(ctx), 0, 0, result);
  return result;
}

int mbedtls_sha256(const unsigned char* input, size_t ilen, unsigned char* output, int is224) {
  std::int32_t result = injected(NEWT_FAKE_SHA_ONESHOT);
  if (result) scribble(output);
  else if (is224 || !newt::host::sha256(input, ilen, output)) result = kRefused;
  record(NEWT_FAKE_SHA_ONESHOT, 0, ilen, is224, result);
  return result;
}

void newt_fake_sha_fail_next(uint32_t op, int32_t code) {
  model().failOp = op;
  model().failCode = code;
}

int newt_fake_sha_take(newt_fake_sha_event* event) {
  auto& events = model().events;
  if (events.empty()) return 0;
  *event = events.front();
  events.pop_front();
  return 1;
}
}
