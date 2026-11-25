/* SPDX-License-Identifier: MIT */
#ifndef NEWT_C3_BOARD_H
#define NEWT_C3_BOARD_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Private ABI, not SDK layout. Sole initialized objects and fixed X3 ACTIVE;
 * no aliases/hooks/reentry. UI exclusively owns display, SPI/SD and Wire; after
 * nb_input_begin input authority transfers once to InitializedInput/TaskPorts.
 * Input task may run during blocking display; it owns InputManager and ADC/GPIO,
 * never SPI/SD/Wire. Compile with identical X3-only FREEINK_CAP_TOUCH=0 flags
 * for this TU, input bridge AND SDK InputManager.cpp. SDK default-null hook must
 * remain null; no hook installation/mutation, input begin/reconfiguration or
 * ACTIVE mutation after task publication. No callback behavior is erased here.
 * Caller retains native queue/gate/counters until task death. No pointer retained
 * by this bridge. No foreign unwind; no overlapping object/output storage.
 * Every boot: select_x3 -> release_sd -> probe -> begin -> mount_sd -> input_begin.
 * No source/parser/frame loans across display/power; stop tasks before power. */
enum { NB_OK = 0, NB_NULL = 1, NB_FRAME = 2, NB_MODE = 3, NB_IO = 4, NB_LENGTH = 5 };
struct nb_frame { uint8_t *data; size_t len; uint16_t width, height, stride; };
struct nb_probe { uint8_t ver[3], verdict, valid, timed_out, promoted; };
struct nb_boot { uint32_t reset, wake, power_on, deep_sleep_reset; };
int32_t nb_select_x3(void *display);
void nb_release_sd(void);
int32_t nb_probe_panel(struct nb_probe *out);
int32_t nb_begin(void *display);
/* Idle bus only; pinned shared-SPI setup, no formatting, three VFS slots.
 * A successful mount attempts Update folders; their errors don't change status. */
int32_t nb_mount_sd(void);
int32_t nb_frame(void *display, struct nb_frame *out);
struct nb_storage { uint8_t *data; size_t len; };
/* Synchronous exclusive primary-frame loan. NB_OK alone transfers the loan;
 * every other result leaves no loan outstanding and clears out. Caller MUST
 * return once, after all byte/typed views and callbacks end, before any other
 * display operation. Returning clears pixels to native white. No allocation.
 * No exception/longjmp may cross this ABI. Raw callers enforce this protocol. */
int32_t nb_lend_build(void *display, struct nb_storage *out);
void nb_return_build(void *display);
/* Blocking only: mode 0 full, 1 half, 2 fast. No async shadow allocation.
 * Success means SDK returned, not a measured successful physical refresh. */
int32_t nb_update(void *display, uint32_t mode, uint32_t turn_off);
int32_t nb_panel_sleep(void *display);
int32_t nb_input_begin(void *input);
uint32_t nb_millis(void);
uint32_t nb_micros(void);
/* Existing initialized global Wire, exclusively borrowed with board resources.
 * X3 DS3231 raw registers 0x00..0x0f, exact length 16. Register-pointer write
 * only; no clock/control writes or validity/time-zone policy. Valid-length
 * transport failures zero output; invalid lengths leave it unchanged. Caller
 * owns sensor-bus initialization, sequential with gauge. */
int32_t nb_clock_read(uint8_t *out, size_t len);
/* Same validation, borrow and transaction (clock_transaction.h). Only NB_OK is
 * a complete snapshot. On a valid-length NB_IO, output keeps the bytes the
 * failed read initialized and zero after them, for Home's DEV line only. */
int32_t nb_clock_snapshot(uint8_t *out, size_t len);
int32_t nb_boot(struct nb_boot *out);
/* Same pinned SDK GPIO release wait; caller has excluded input polling. */
void nb_wait_power_release(void);
/* Availability only: SDK discards IDF arm error. NOT confirmed wake arming. */
uint32_t nb_arm_power_wake(void);
void nb_power_down_rails(void);
/* Only after app finished paint, durable commits, released button, closed IO,
 * stopped tasks and panel/rails. Success never returns; rejection returns raw
 * esp_err_t. App owns recovery/restart, not this boundary. */
int32_t nb_try_sleep(void);
void nb_restart(void);
#ifdef __cplusplus
}
#endif
#endif
