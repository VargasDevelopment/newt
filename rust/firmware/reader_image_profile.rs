// SPDX-License-Identifier: MIT
//! Short file/hash/terminal observations share the open's one UI stack profile.
use newt_core::reader_work_profile::{ReaderWorkProfile, ReaderIoOperation, ReaderHashOperation,
    ReaderOpenTerminal, TraceSink, trace_reader_details, trace_reader_work};
use newt_c3_board::MonotonicClock;
use crate::{reader_selected_source::FileObserver, reader_hash::SourceHashObserver,
    serial_console::SerialConsole};

#[cfg(feature = "dev-usb")]
pub(super) fn with_profile<R>(work: impl FnOnce(Option<&ReaderWorkProfile>) -> R) -> R {
    let profile = ReaderWorkProfile::default();
    work(Some(&profile))
}
#[cfg(not(feature = "dev-usb"))]
pub(super) fn with_profile<R>(work: impl FnOnce(Option<&ReaderWorkProfile>) -> R) -> R { work(None) }

pub(super) struct Observations<'a> { pub(super) profile: &'a ReaderWorkProfile, pub(super) clock: MonotonicClock }
impl FileObserver for Observations<'_> {
    fn micros(&self) -> u32 { self.clock.micros() }
    fn io(&self, operation: ReaderIoOperation, elapsed: u32, bytes: u32) { self.profile.io(operation, elapsed, bytes); }
}
impl SourceHashObserver for Observations<'_> {
    fn micros(&self) -> u32 { self.clock.micros() }
    fn hash(&self, operation: ReaderHashOperation, elapsed: u32, bytes: u32) { self.profile.hash(operation, elapsed, bytes); }
}
impl TraceSink for SerialConsole {
    fn available_for_write(&mut self) -> i32 { newt_c3_board::dev_usb::available_for_write() }
    fn write(&mut self, bytes: &[u8]) -> usize {
        self.write_bytes(bytes)
    }
}
pub(super) fn finish(runtime: &super::Runtime, profile: &ReaderWorkProfile,
    snapshot: newt_input_policy::Snapshot, log: &mut SerialConsole,
) {
    let status = runtime.has_progress().then(|| runtime.status());
    let ready = runtime.opened();
    profile.terminal(if status.is_some() { ReaderOpenTerminal::Reader }
        else { ReaderOpenTerminal::SourceUnavailable }, status, ready,
        &snapshot.elapsed, &snapshot.reason, &snapshot.active);
    profile.finish_observation();
    let emitted = trace_reader_work(log, profile);
    trace_reader_details(log, profile, emitted);
}
