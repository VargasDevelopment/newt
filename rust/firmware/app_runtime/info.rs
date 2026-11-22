// SPDX-License-Identifier: MIT
//! The one frozen readDeviceInfo body, shared by the selected boot mode.
#![forbid(unsafe_code)]
use newt_c3_board::{MonotonicClock, hardware::{HardwareError, HardwareReader}};
use newt_device_ui::DeviceInfo;

pub fn read_device_info(info: &mut DeviceInfo<'_>, hardware: &mut HardwareReader<'_>, clock: &MonotonicClock) -> Result<u32, HardwareError> {
    let status = hardware.battery_status()?;
    info.battery_known = status.percentage_known && status.percentage <= 100;
    info.battery_percent = if info.battery_known { status.percentage } else { 0 };
    info.millivolts_known = status.millivolts_known;
    info.millivolts = status.millivolts;
    info.charging_known = status.charging_known;
    info.charging = status.charging;
    info.free_heap = hardware.free_heap_bytes()?;
    Ok(clock.millis())
}
