// SPDX-License-Identifier: MIT
#include "task_owner.h"
#include "atomic_bridge.h"
#include "x3_input_compile_contract.h"
#include <freertos/FreeRTOS.h>
#include <freertos/queue.h>
#include <freertos/task.h>
#include <stdlib.h>
#include "press_event.h"
#if NEWT_DEV_USB
#include "dev_gate.h"
using PressEvent = newt::dev::ButtonEvent;
#else
using PressEvent = uint8_t;
#endif
static_assert(sizeof(PressEvent) == (NEWT_DEV_USB ? 8 : 1));
namespace {
struct Owner {
    np_atomic_u32 state;
    np_atomic_u32 dropped;
#if NEWT_DEV_USB
    np_dev_gate gate;
#endif
    QueueHandle_t queue = nullptr;
    TaskHandle_t task = nullptr;
    Owner() = default;
    Owner(const Owner&) = delete;
    Owner& operator=(const Owner&) = delete;
    ~Owner() {
        // No cleanup can revoke the running task's Rust or native references.
        if (task) abort();
        if (queue) vQueueDelete(queue);
    }
};
Owner& owner(void *pointer) { return *static_cast<Owner*>(pointer); }
uint32_t withOwner(np_owner_body body, void *argument, bool createQueue) noexcept {
    Owner storage; // Its address never escapes this nonmoving lexical scope.
    if (createQueue && !np_owner_create_queue(&storage)) return 1;
    const uint32_t result = body(&storage, argument);
    // Defence against an invalid foreign callback returning after launch.
    // Rust also forbids unwinding before this point: this guard cannot repair it.
    if (storage.task) abort();
    return result;
}
}
uint32_t np_owner_with(np_owner_body body, void *argument) noexcept {
    return withOwner(body, argument, true);
}
uint32_t np_owner_with_deferred(np_owner_body body, void *argument) noexcept {
    return withOwner(body, argument, false);
}
uint8_t np_owner_create_queue(void *p) noexcept {
    auto& value = owner(p);
    if (value.queue || value.task) abort();
    value.queue = xQueueCreate(32, sizeof(PressEvent));
    return value.queue != nullptr;
}
void *np_owner_queue(void *p) noexcept { return owner(p).queue; }
void *np_owner_gate(void *p) noexcept {
#if NEWT_DEV_USB
    return &owner(p).gate;
#else
    (void)p;
    return nullptr;
#endif
}
np_atomic_u32 *np_owner_state(void *p) noexcept { return &owner(p).state; }
np_atomic_u32 *np_owner_dropped(void *p) noexcept { return &owner(p).dropped; }
uint8_t np_owner_start(void *p, np_task_entry entry, void *context) noexcept {
    auto& value = owner(p);
    if (value.task) abort();
    // ESP-IDF's retained xTaskCreate takes BYTES, not upstream stack words.
    // task/context may be accessed before this call returns; publish first.
    return xTaskCreate(entry, "newt_buttons", 4096, context, 2, &value.task) == pdPASS;
}
uint8_t np_owner_suspend(void *p) noexcept {
    auto& value = owner(p);
    if (!value.task) return 0;
    vTaskSuspend(value.task);
    return 1;
}
void np_owner_resume(void *p) noexcept {
    auto& value = owner(p);
    if (value.task) vTaskResume(value.task);
}
