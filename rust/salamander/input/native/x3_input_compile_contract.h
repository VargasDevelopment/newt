/* SPDX-License-Identifier: MIT */
#ifndef NEWT_X3_INPUT_COMPILE_CONTRACT_H
#define NEWT_X3_INPUT_COMPILE_CONTRACT_H
#include <BoardConfig.h>
#if !FREEINK_DEVICE_X3 || FREEINK_DEVICE_X4 || FREEINK_CAP_TOUCH || !FREEINK_BATTERY_I2C_GAUGE
#error "Newt split input ownership requires X3-only, no touch, I2C gauge SDK build"
#endif
#endif
