// SPDX-License-Identifier: MIT
//! Frozen network_clock.h policy. All bus calls use the caller's sole Board;
//! Core supplies the existing plausibility, calendar, trust and wire helpers.
#![forbid(unsafe_code)]
use newt_c3_board::Board;
use newt_core::{ds3231::{clock_bcd, plausible_clock}, network_clock::{self, NetworkTime}, time_zone::ClockTime};

#[derive(Clone, Copy)]
pub enum Result { NoDate, Untrusted, Unread, Close, Written, NotWritten, Mismatch, OsfKept }
pub struct Report { pub result: Result, pub utc: u32, pub drift: i64, pub chip_valid: bool, pub osf: i32 }

fn clock_seconds(raw: &[u8; 16]) -> Option<u32> {
    let hour = plausible_clock(raw)?;
    if raw[5] & 0x80 != 0 { return None; }
    Some(network_clock::utc_seconds(ClockTime { year: 2000 + u16::from(clock_bcd(raw[6])),
        month: clock_bcd(raw[5] & 0x1f), day: clock_bcd(raw[4]), hour, minute: clock_bcd(raw[1]) },
        u32::from(clock_bcd(raw[0]))))
}
fn clock_matches(raw: &[u8; 16], utc: u32) -> bool {
    raw[2] & 0x40 == 0 && clock_seconds(raw).is_some_and(|seconds| seconds.wrapping_sub(utc) <= 1)
}

pub fn set(board: &mut Board<'_>, time: NetworkTime, floor: u32, now_ms: u32) -> Report {
    let mut report = Report { result: Result::NoDate, utc: 0, drift: 0, chip_valid: false, osf: -1 };
    if time.seconds == 0 { return report; }
    report.utc = time.seconds.wrapping_add(now_ms.wrapping_sub(time.at_ms).wrapping_add(500) / 1000);
    report.result = Result::Untrusted;
    if !network_clock::trusted_network_time(report.utc, floor) { return report; }
    report.result = Result::Unread;
    let snapshot = board.clock_snapshot();
    if !snapshot.complete { return report; }
    report.osf = i32::from(snapshot.raw[15] & 0x80 != 0);
    if report.osf == 0 {
        if let Some(chip) = clock_seconds(&snapshot.raw) {
            report.chip_valid = true;
            report.drift = i64::from(chip) - i64::from(report.utc);
            report.result = Result::Close;
            if (-2..=2).contains(&report.drift) { return report; }
        }
    }
    let mut registers = [0; 7];
    network_clock::encode_clock_registers(report.utc, &mut registers);
    report.result = Result::NotWritten;
    let pending = [0, registers[6]];
    if !board.write_clock_registers(5, &pending) || !board.write_clock_registers(0, &registers[..5])
        || !board.write_clock_registers(5, &registers[5..6]) { return report; }
    report.result = Result::Mismatch;
    let snapshot = board.clock_snapshot();
    if !snapshot.complete || !clock_matches(&snapshot.raw, report.utc) { return report; }
    if snapshot.raw[15] & 0x80 != 0 {
        let status = (snapshot.raw[15] & 0x7c) | 3;
        report.result = Result::OsfKept;
        if !board.write_clock_registers(15, &[status]) { return report; }
        let snapshot = board.clock_snapshot();
        if !snapshot.complete || snapshot.raw[15] & 0x80 != 0 || !clock_matches(&snapshot.raw, report.utc) { return report; }
    }
    report.result = Result::Written;
    report
}
pub fn trace(log: &mut impl core::fmt::Write, report: &Report) {
    #[cfg(feature = "dev-usb")]
    {
        let name = match report.result { Result::NoDate => "no-date", Result::Untrusted => "untrusted",
            Result::Unread => "unread", Result::Close => "close", Result::Written => "written",
            Result::NotWritten => "not-written", Result::Mismatch => "mismatch", Result::OsfKept => "osf-kept" };
        let _ = writeln!(log, "CLOCK reason=network result={} utc={} valid={} drift={} osf={}",
            name, report.utc, u8::from(report.chip_valid), report.drift, report.osf);
    }
    #[cfg(not(feature = "dev-usb"))]
    let _ = (log, report);
}
