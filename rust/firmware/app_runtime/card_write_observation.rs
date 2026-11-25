// SPDX-License-Identifier: MIT
//! Numbers held only during a granted DEV PUT, never paths or payloads.
#![forbid(unsafe_code)]
use core::fmt::{self, Write};
use newt_core::dev_put::{ExitReason, Observation, Wire};

/// Best effort within the existing output budget. HWCDC can briefly report
/// disconnected; allow recovery instead of silently dropping the exit row.
pub fn report(wire: &mut dyn Wire, bytes: &[u8]) {
    let mut sent = 0;
    let start = wire.now();
    while sent < bytes.len() && wire.now().wrapping_sub(start) < 1_000 {
        if !wire.connected() { wire.pause(); continue; }
        let put = wire.write(&bytes[sent..]);
        if put > bytes.len() - sent { break; }
        sent += put;
        if put == 0 { wire.pause(); }
    }
}

#[derive(Default)]
pub struct Counters {
    reason: Option<ExitReason>,
    id: u32,
    offset: u64,
    expected: u32,
    received: u32,
    bytes: u64,
    empty: u32,
    pauses: u32,
    pause_ms: u32,
    rx_ms: u32,
    write_ms: u32,
    ack_ms: u32,
    commit_ms: u32,
    phase: u8,
    since: u32,
}
impl Counters {
    fn account(&mut self, now: u32) {
        let elapsed = now.wrapping_sub(self.since);
        let total = match self.phase {
            1 => &mut self.rx_ms, 2 => &mut self.write_ms,
            3 => &mut self.ack_ms, 4 => &mut self.commit_ms,
            _ => { self.since = now; return; }
        };
        *total = total.saturating_add(elapsed);
        self.since = now;
    }
    pub fn observe(&mut self, now: u32, event: Observation) {
        if let Observation::Request(id) = event { self.id = id; return; }
        self.account(now);
        self.phase = match event {
            Observation::Receiving { offset, expected } => {
                self.offset = offset; self.expected = expected; self.received = 0; 1
            }
            Observation::Writing => 2,
            Observation::Reply => 3,
            Observation::Commit => 4,
            Observation::Exit(reason) => { self.reason.get_or_insert(reason); 0 }
            _ => 0,
        };
    }
    pub fn read(&mut self, byte: i32) {
        if byte < 0 { self.empty = self.empty.saturating_add(1); }
        else {
            self.bytes += 1;
            if self.phase == 1 { self.received += 1; }
        }
    }
    pub fn paused(&mut self, elapsed: u32) {
        self.pauses = self.pauses.saturating_add(1);
        self.pause_ms = self.pause_ms.saturating_add(elapsed);
    }
    pub fn row(&mut self, now: u32, output: &mut impl Write) -> fmt::Result {
        self.account(now);
        // A failed protocol reply may have left a fragment without its LF.
        writeln!(output, "\nPUT_STATS reason={} id={} offset={} received={} expected={} bytes={} empty={} pauses={} pause_ms={} rx_ms={} write_ms={} ack_ms={} commit_ms={}",
            self.reason.map_or(255, |reason| reason as u8), self.id, self.offset, self.received,
            self.expected, self.bytes, self.empty, self.pauses, self.pause_ms,
            self.rx_ms, self.write_ms, self.ack_ms, self.commit_ms)
    }
}
