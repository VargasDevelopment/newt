// SPDX-License-Identifier: MIT
// SDK primitives only. Rust owns deadlines, headers, allocator routing and loans.
#include <Arduino.h>
#include <esp_crt_bundle.h>
#include <esp_http_client.h>
#include <esp_timer.h>
#include <mbedtls/platform.h>
#include <multi_heap.h>
#include <lwip/sockets.h>
#include <arch/sys_arch.h>
#include <cstddef>
#include <cstdint>
#include <new>

#ifndef NEWT_CRT_BUNDLE_PATH
#error "NEWT_CRT_BUNDLE_PATH must name the Root-pinned existing certificate bundle"
#endif
asm(".section .rodata.newt_crt_bundle,\"a\"\n"
    ".balign 4\n"
    ".global _binary_x509_crt_bundle_start\n"
    "_binary_x509_crt_bundle_start:\n"
    ".incbin \"" NEWT_CRT_BUNDLE_PATH "\"\n"
    ".global _binary_x509_crt_bundle_end\n"
    "_binary_x509_crt_bundle_end:\n.previous\n");

extern "C" void newt_https_note_header(void*, const char*, const char*);
static esp_err_t headerEvent(esp_http_client_event_t* event) {
  if (event->event_id==HTTP_EVENT_ON_HEADER && event->user_data && event->header_key && event->header_value)
    newt_https_note_header(event->user_data,event->header_key,event->header_value);
  return ESP_OK;
}
extern "C" void* nh_http_init(const char* url,std::uint32_t timeout) {
  esp_http_client_config_t config{};
  config.url=url;
  config.timeout_ms=static_cast<int>(timeout);
  config.disable_auto_redirect=true;
  config.buffer_size=1024;
  config.buffer_size_tx=2048+512;
  config.user_agent="Newt X3";
  config.event_handler=headerEvent;
  config.crt_bundle_attach=esp_crt_bundle_attach;
  return esp_http_client_init(&config);
}
extern "C" bool nh_http_set_header(void* client,const char* key,const char* value) {
  return esp_http_client_set_header(static_cast<esp_http_client_handle_t>(client),key,value)==ESP_OK;
}
extern "C" bool nh_http_open(void* client) {
  return esp_http_client_open(static_cast<esp_http_client_handle_t>(client),0)==ESP_OK;
}
extern "C" bool nh_http_set_context(void* client,void* context) {
  return esp_http_client_set_user_data(static_cast<esp_http_client_handle_t>(client),context)==ESP_OK;
}
extern "C" std::int64_t nh_http_headers(void* client) {
  return esp_http_client_fetch_headers(static_cast<esp_http_client_handle_t>(client));
}
extern "C" int nh_http_status(void* client) { return esp_http_client_get_status_code(static_cast<esp_http_client_handle_t>(client)); }
extern "C" int nh_http_read(void* client,std::uint8_t* bytes,int count) {
  return esp_http_client_read(static_cast<esp_http_client_handle_t>(client),reinterpret_cast<char*>(bytes),count);
}
extern "C" bool nh_http_complete(void* client) {
  return esp_http_client_is_complete_data_received(static_cast<esp_http_client_handle_t>(client));
}
extern "C" void nh_http_close(void* client) { esp_http_client_close(static_cast<esp_http_client_handle_t>(client)); }
extern "C" void nh_http_cleanup(void* client) { esp_http_client_cleanup(static_cast<esp_http_client_handle_t>(client)); }
extern "C" void* nh_timer_create(esp_timer_cb_t callback,void* deadline) {
  esp_timer_create_args_t args{};
  args.callback=callback;
  args.arg=deadline;
  args.name="newt_https";
  esp_timer_handle_t timer=nullptr;
  return esp_timer_create(&args,&timer)==ESP_OK ? timer : nullptr;
}
extern "C" bool nh_timer_start(void* timer,std::uint64_t interval) {
  return esp_timer_start_periodic(static_cast<esp_timer_handle_t>(timer),interval)==ESP_OK;
}
extern "C" void nh_timer_stop(void* timer) { esp_timer_stop(static_cast<esp_timer_handle_t>(timer)); }
extern "C" void nh_timer_delete(void* timer) { esp_timer_delete(static_cast<esp_timer_handle_t>(timer)); }
extern "C" int nh_socket_first() { return LWIP_SOCKET_OFFSET; }
extern "C" int nh_socket_count() { return CONFIG_LWIP_MAX_SOCKETS; }
extern "C" void nh_socket_shutdown(int fd) { lwip_shutdown(fd,SHUT_RDWR); }
extern "C" void nh_socket_sem_deinit() { sys_thread_sem_deinit(); }

struct alignas(16) TlsFrameLoan {
  multi_heap_handle_t heap;
  TaskHandle_t owner;
  portMUX_TYPE lock;
};
static_assert(alignof(TlsFrameLoan)==16);
static_assert(sizeof(TlsFrameLoan)%16==0);
extern "C" std::size_t nh_tls_header_bytes() { return sizeof(TlsFrameLoan); }
extern "C" void nh_tls_construct(void* header) { new(header) TlsFrameLoan{}; }
extern "C" bool nh_tls_register(void* header,void* start,std::size_t bytes) {
  auto* loan=static_cast<TlsFrameLoan*>(header);
  loan->heap=multi_heap_register(start,bytes);
  return loan->heap!=nullptr;
}
extern "C" void nh_tls_bind_task_lock(void* header) {
  auto* loan=static_cast<TlsFrameLoan*>(header);
  portMUX_INITIALIZE(&loan->lock);
  multi_heap_set_lock(loan->heap,&loan->lock);
  loan->owner=xTaskGetCurrentTaskHandle();
}
extern "C" bool nh_tls_owner_is_current(void* header) { return xTaskGetCurrentTaskHandle()==static_cast<TlsFrameLoan*>(header)->owner; }
extern "C" void* nh_tls_malloc(void* header,std::size_t bytes) { return multi_heap_malloc(static_cast<TlsFrameLoan*>(header)->heap,bytes); }
extern "C" void nh_tls_free(void* header,void* bytes) { multi_heap_free(static_cast<TlsFrameLoan*>(header)->heap,bytes); }
extern "C" void nh_tls_info(void* header,std::size_t* allocated,std::size_t* lowest) {
  multi_heap_info_t info{};
  multi_heap_get_info(static_cast<TlsFrameLoan*>(header)->heap,&info);
  *allocated=info.allocated_blocks;
  *lowest=info.minimum_free_bytes;
}
// The pinned mbedTLS header declares these C symbols without a C++ linkage guard.
void* defaultCalloc(std::size_t,std::size_t) asm("esp_mbedtls_mem_calloc");
void defaultFree(void*) asm("esp_mbedtls_mem_free");
extern "C" int nh_tls_install(void* (*callocFn)(std::size_t,std::size_t),void (*freeFn)(void*)) {
  return mbedtls_platform_set_calloc_free(callocFn,freeFn);
}
extern "C" int nh_tls_restore() { return mbedtls_platform_set_calloc_free(defaultCalloc,defaultFree); }
extern "C" void* nh_tls_default_calloc(std::size_t count,std::size_t size) { return defaultCalloc(count,size); }
extern "C" void nh_tls_default_free(void* bytes) { defaultFree(bytes); }
