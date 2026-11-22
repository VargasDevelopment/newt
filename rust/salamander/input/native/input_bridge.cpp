// SPDX-License-Identifier: MIT
#include "input_bridge.h"
#include <InputManager.h>
#include "x3_input_compile_contract.h"
#include <freertos/FreeRTOS.h>
#include <freertos/queue.h>
#include <freertos/task.h>
#include "press_event.h"
#if NEWT_DEV_USB
#include "dev_gate.h"
using PressEvent = newt::dev::ButtonEvent;
#else
using PressEvent = uint8_t;
#endif
static_assert(InputManager::BTN_BACK == 0 && InputManager::BTN_CONFIRM == 1 &&
    InputManager::BTN_LEFT == 2 && InputManager::BTN_RIGHT == 3 &&
    InputManager::BTN_UP == 4 && InputManager::BTN_DOWN == 5 && InputManager::BTN_POWER == 6);
void np_update(void *input) { static_cast<InputManager*>(input)->update(); }
uint8_t np_physical_power(void *input) {
    return static_cast<InputManager*>(input)->isPowerButtonPhysicallyPressed();
}
uint8_t np_logical_power(void *input) {
    return static_cast<InputManager*>(input)->isPressed(InputManager::BTN_POWER);
}
uint8_t np_pressed(void *input, uint8_t button) {
    return static_cast<InputManager*>(input)->wasPressed(button);
}
#if NEWT_DEV_USB
bool *np_gate_enter(void *pointer) {
    return np_port_gate_enter(*static_cast<np_dev_gate*>(pointer));
}
void np_gate_exit(void *pointer) {
    np_port_gate_exit(*static_cast<np_dev_gate*>(pointer));
}
#endif
static PressEvent eventFor(uint8_t button) {
#if NEWT_DEV_USB
    return {0, button};
#else
    return button;
#endif
}
uint8_t np_tail(void *queue, uint8_t button) {
    const auto event = eventFor(button);
    return xQueueSend(static_cast<QueueHandle_t>(queue), &event, 0) == pdTRUE;
}
uint8_t np_front_wait(void *queue, uint8_t button) {
    const auto event = eventFor(button);
    return xQueueSendToFront(static_cast<QueueHandle_t>(queue), &event, portMAX_DELAY) == pdTRUE;
}
static void exportEvent(const PressEvent& event, struct np_event *out) {
#if NEWT_DEV_USB
    *out = {event.ticket, event.button};
#else
    *out = {0, event};
#endif
}
uint8_t np_receive(void *queue, struct np_event *out) {
    PressEvent event;
    if (xQueueReceive(static_cast<QueueHandle_t>(queue), &event, 0) != pdTRUE) return 0;
    exportEvent(event, out);
    return 1;
}
uint8_t np_peek(void *queue, struct np_event *out) {
    PressEvent event;
    if (xQueuePeek(static_cast<QueueHandle_t>(queue), &event, 0) != pdTRUE) return 0;
    exportEvent(event, out);
    return 1;
}
uint8_t np_pending(void *queue) {
    return uxQueueMessagesWaiting(static_cast<QueueHandle_t>(queue)) != 0;
}
void np_delay_ms(uint32_t ms) { vTaskDelay(pdMS_TO_TICKS(ms)); }

#if NEWT_DEV_USB
uint8_t np_send_ticket(void *queue, uint32_t ticket, uint8_t button) {
    const PressEvent event{ticket, button};
    return xQueueSend(static_cast<QueueHandle_t>(queue), &event, 0) == pdTRUE;
}
#endif
