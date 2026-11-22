// SPDX-License-Identifier: MIT
//! Final UI-task power boundary. Reader commits, records and closure stay in
//! the required preparation callback while input remains suspended.
use crate::{app_image::Home, retained_runtime::record::{SleepRecord, stamp_rejection}};
use core::fmt::Write;
use newt_input_policy::native::QuiescedInput;

// SAFETY: matching native entry forwards Arduino delay on the current task.
unsafe extern "C" { fn nb_delay_ms(ms: u32); }
pub fn delay_ms(ms: u32) {
    // SAFETY: the SDK scheduler exists throughout the application's UI loop.
    unsafe { nb_delay_ms(ms) }
}

/// A real native failure cannot return into a partially updated application.
pub fn stop() -> ! { crate::stop() }

/// The caller's actual reader preparation closes its source/session and all
/// derived I/O before returning the record. This fixed image boundary never
/// resumes the task after rails or wake configuration have been committed.
pub(crate) fn enter_sleep_quiesced<'sdk>(home: &mut Home<'sdk>, input: &QuiescedInput<'_>,
    prepare: &mut dyn FnMut(&mut Home<'sdk>) -> SleepRecord,
) -> bool {
    // SAFETY: the caller holds the actual suspended input task token. No
    // frame, GPIO or parser loan exists at this application tail boundary.
    unsafe { home.bus.board.wait_power_release(input); }
    // SAFETY: the same physical-input barrier remains live through reset.
    if !unsafe { home.bus.board.arm_power_wake_pin_available(input) } { return false; }
    let mut retained = prepare(home);
    let _ = home.write_retained_sleep(&retained);
    let _ = home.bus.board.panel_sleep();
    // SAFETY: the image's actual prepare callback closed every selected
    // file, optional session and storage/parse loan before this point.
    unsafe { home.bus.board.power_down_rails(input); }
    home.bus.log.flush();
    enter_prepared_sleep(home, input, &mut retained)
}

/// Frozen enterDeepSleep's named, straight-line try/stamp/restart boundary.
/// Keep it out of line so the image gate can verify every required edge.
#[inline(never)]
fn enter_prepared_sleep(home: &mut Home<'_>, input: &QuiescedInput<'_>, retained: &mut SleepRecord) -> ! {
    // SAFETY: task stays suspended; wake is armed, records/IO are closed,
    // the button was released and the SDK panel/rails have powered down.
    let error = unsafe { home.bus.board.try_sleep(input) };
    stamp_rejection(retained);
    store_rejection(home, retained, error);
    home.bus.board.restart();
    stop()
}

/// The existing RTC write and diagnostics, after rejection was stamped and
/// before restart. This holds no I/O/source or additional record owner.
#[inline(never)]
fn store_rejection(home: &mut Home<'_>, retained: &SleepRecord, error: i32) {
    let _ = home.write_retained_sleep(retained);
    let _ = writeln!(home.bus.log, "Newt sleep entry returned: error={}; restarting to saved route", error);
    home.bus.log.flush();
}
