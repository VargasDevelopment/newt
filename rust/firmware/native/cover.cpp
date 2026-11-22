// SPDX-License-Identifier: MIT
// Thin pinned SDK primitives. Cover dispatch, ownership and publication are Rust.
#include <cstddef>
#include <cstdint>
#include <cerrno>
#include <limits>
#include <sys/stat.h>
#include <esp_heap_caps.h>
#include <esp_idf_version.h>
#include <sdkconfig.h>
#include <expat.h>
#include <freertos/FreeRTOS.h>
#include <freertos/task.h>
extern "C" void* newt_cover_xml_user(XML_Parser parser) { return XML_GetUserData(parser); }
extern "C" int newt_cover_heap_monitor_start() {
  return heap_caps_monitor_local_minimum_free_size_start();
}
extern "C" int newt_cover_heap_monitor_stop() {
  return heap_caps_monitor_local_minimum_free_size_stop();
}
extern "C" std::size_t newt_cover_heap_monitor_minimum() {
  return heap_caps_get_minimum_free_size(MALLOC_CAP_INTERNAL|MALLOC_CAP_8BIT);
}
extern "C" std::size_t newt_cover_heap_monitor_largest() {
  return heap_caps_get_largest_free_block(MALLOC_CAP_INTERNAL|MALLOC_CAP_8BIT);
}
extern "C" std::uint32_t newt_cover_stack_min_free() {
  return static_cast<std::uint32_t>(uxTaskGetStackHighWaterMark(nullptr));
}
struct CoverFacts {
  std::int64_t size; std::uint64_t inode,device; std::int64_t seconds;
  std::uint64_t links; std::uint32_t mode;
};
static_assert(sizeof(CoverFacts)==48);
static_assert(alignof(std::max_align_t)==16,"Rust cover allocation header requires pinned max alignment");
extern "C" bool newt_cover_complete_handle_metadata() {
#if defined(ESP_PLATFORM) || defined(ARDUINO_ARCH_ESP32)
  return false;
#else
  return true;
#endif
}
extern "C" bool newt_cover_rename_replaces_destination() {
#if defined(ESP_PLATFORM) || defined(ARDUINO_ARCH_ESP32)
  return false;
#else
  return true;
#endif
}
static int result(int value) {
  if (!value) return 0;
  if (errno==EINTR) return 1;
  if (errno==ENOENT) return 2;
  if (errno==EEXIST) return 3;
  if (errno==EIO || errno==ENODEV || errno==ENXIO) return 6;
  return 5;
}
static void copy(CoverFacts* out,const struct stat& info) {
  *out={static_cast<std::int64_t>(info.st_size),static_cast<std::uint64_t>(info.st_ino),
        static_cast<std::uint64_t>(info.st_dev),static_cast<std::int64_t>(info.st_mtime),
        static_cast<std::uint64_t>(info.st_nlink),static_cast<std::uint32_t>(info.st_mode)};
}
extern "C" int newt_cover_stat(const char* path,CoverFacts* out) {
  struct stat info{};
#if defined(ESP_PLATFORM) || defined(ARDUINO_ARCH_ESP32)
  const auto status=::stat(path,&info);
#else
  const auto status=::lstat(path,&info);
#endif
  if (!status) copy(out,info); return result(status);
}
extern "C" int newt_cover_fstat(int fd,CoverFacts* out) {
  struct stat info{};const auto status=::fstat(fd,&info);
  if (!status) copy(out,info);return result(status);
}
static constexpr auto caps=MALLOC_CAP_INTERNAL|MALLOC_CAP_8BIT;
extern "C" bool newt_cover_heap_supported() {
#if ESP_IDF_VERSION == ESP_IDF_VERSION_VAL(5,5,2) && defined(CONFIG_IDF_TARGET_ESP32C3) && \
    defined(CONFIG_HEAP_POISONING_LIGHT) && CONFIG_HEAP_POISONING_LIGHT && \
    defined(CONFIG_HEAP_TRACING_OFF) && CONFIG_HEAP_TRACING_OFF && \
    !defined(CONFIG_HEAP_TASK_TRACKING) && !defined(CONFIG_HEAP_ABORT_WHEN_ALLOCATION_FAILS)
  return sizeof(void*)==4 && alignof(std::max_align_t)<=16;
#else
  return false;
#endif
}
extern "C" std::size_t newt_cover_heap_free_bytes() {
  return newt_cover_heap_supported() ? heap_caps_get_free_size(caps) : 0;
}
extern "C" std::size_t newt_cover_heap_largest() {
  return newt_cover_heap_supported() ? heap_caps_get_largest_free_block(caps) : 0;
}
extern "C" std::size_t newt_cover_heap_charged(void* block) {
  if (!newt_cover_heap_supported() || !block) return std::numeric_limits<std::size_t>::max();
  const auto usable=heap_caps_get_allocated_size(block);
  return usable>std::numeric_limits<std::size_t>::max()-16 ?
      std::numeric_limits<std::size_t>::max() : usable+16;
}
extern "C" void newt_cover_heap_release(void* block) { heap_caps_free(block); }
extern "C" void* newt_cover_heap_allocate(std::size_t bytes) {
  constexpr std::size_t reserve=64*1024,allowance=128;
  if (!newt_cover_heap_supported() || !bytes || bytes>64*1024 ||
      bytes>std::numeric_limits<std::size_t>::max()-allowance-reserve) return nullptr;
  const auto ceiling=bytes+allowance;
  if (newt_cover_heap_free_bytes()<ceiling+reserve || newt_cover_heap_largest()<ceiling) return nullptr;
  void* block=heap_caps_aligned_alloc(alignof(std::max_align_t),bytes,caps);
  if (!block) return nullptr;
  const auto actual=newt_cover_heap_charged(block);
  if (actual<bytes || actual>ceiling || newt_cover_heap_free_bytes()<reserve ||
      reinterpret_cast<std::uintptr_t>(block)%alignof(std::max_align_t)) {
    newt_cover_heap_release(block);return nullptr;
  }
  return block;
}
