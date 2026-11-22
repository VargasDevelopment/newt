// SPDX-License-Identifier: MIT
//! The frozen N1 development protocol. No SDK, task, allocation or frame copy.
//! The boot's immutable build string is lent to poll rather than stored twice.
#![forbid(unsafe_code)]

#[path = "dev_usb/commands.rs"]
mod commands;
#[path = "dev_usb/panel.rs"]
mod panel;
#[path = "dev_usb/text.rs"]
mod text;
#[path = "dev_usb/types.rs"]
mod types;
#[cfg(feature = "dev-usb")]
#[path = "dev_usb/transport.rs"]
mod transport;
pub use panel::{PanelOutcome, PanelRing, PanelToken};
pub use types::{ButtonEvent, Channel, Device, Diagnostics, Status};
use newt_core::wire::crc32;
use core::fmt::Write as _;
use text::Text;

pub const CHUNK_BYTES: usize = 160;
pub const FRAME_BYTES: usize = newt_core::frame::FRAME_BYTES;
pub const PURGE_CACHE_BUTTON: u8 = 0x7e;

/// Sole UI-task protocol state. Physical-input exclusion is supplied by the
/// existing input owner, not by a second bridge or a retained frame reference.
pub struct Bridge {
    rx: [u8; 80],
    tx: [u8; 512],
    boot: u32,
    session: u32,
    session_counter: u32,
    last_id: u32,
    ticket_counter: u32,
    pending_ticket: u32,
    pending_id: u32,
    completed_generation: u32,
    lease_generation: u32,
    lease_start: u32,
    lease_activity: u32,
    session_activity: u32,
    rx_start: u32,
    tx_progress: u32,
    rx_size: u16,
    tx_size: u16,
    tx_offset: u16,
    notification: u8,
    overflow: bool,
    consumed: bool,
    rendered: bool,
    capture: bool,
    refreshed: bool,
    capture_expired: bool,
}

impl Bridge {
    pub const fn new(boot: u32) -> Self {
        Self {
            rx: [0; 80], tx: [0; 512], boot,
            session: 0, session_counter: 0, last_id: 0,
            ticket_counter: 0, pending_ticket: 0, pending_id: 0,
            completed_generation: 0, lease_generation: 0, lease_start: 0,
            lease_activity: 0, session_activity: 0, rx_start: 0, tx_progress: 0,
            rx_size: 0, tx_size: 0, tx_offset: 0, notification: 0,
            overflow: false, consumed: false, rendered: false, capture: false,
            refreshed: false, capture_expired: false,
        }
    }

    fn cancel(&mut self) {
        self.session = 0;
        self.last_id = 0;
        self.rx_size = 0;
        self.overflow = false;
        self.tx_size = 0;
        self.tx_offset = 0;
        self.pending_ticket = 0;
        self.notification = 0;
        self.consumed = false;
        self.rendered = false;
        self.capture = false;
        self.capture_expired = false;
    }

    fn reply(&mut self, id: u32, body: &[u8]) {
        let mut out = Text::new(&mut self.tx);
        let _ = write!(out, "!N1 {} {} {} ", self.boot, self.session, id);
        out.bytes(body);
        let length = out.required();
        if length.saturating_add(12) >= self.tx.len() {
            self.cancel();
            return;
        }
        let checksum = crc32(&self.tx[..length]);
        let mut tail = Text::new(&mut self.tx[length..]);
        let _ = write!(tail, " *{checksum:08x}\n");
        self.tx_size = (length + tail.len()) as u16;
        self.tx_offset = 0;
    }

    pub fn consume(&mut self, ticket: u32, connected: bool, now: u32) -> bool {
        if !connected || self.session == 0 || now.wrapping_sub(self.session_activity) >= 60_000 {
            self.cancel();
            return false;
        }
        if ticket == 0 || ticket != self.pending_ticket || self.consumed {
            return false;
        }
        self.consumed = true;
        true
    }

    pub fn render_returned(&mut self, generation: u32, refreshed: bool) {
        if self.pending_ticket != 0 && self.consumed {
            self.rendered = true;
            self.completed_generation = generation;
            self.refreshed = refreshed;
        }
    }

    pub fn physical_input(&mut self) { self.cancel(); }
    /// Called only while the existing input gate's pending byte is lent. Both
    /// drain and consume share this ordering, including a queued physical press.
    pub fn drain_pending(&mut self, pending: &mut bool) {
        if *pending {
            *pending = false;
            self.physical_input();
        }
    }
    pub fn consume_pending(&mut self, pending: &mut bool, ticket: u32, connected: bool, now: u32) -> bool {
        self.drain_pending(pending);
        self.consume(ticket, connected, now)
    }
    pub fn going_to_sleep(&mut self) { self.capture = false; }
    pub fn housekeeping_done(&mut self) { self.capture_expired = false; }
    pub fn capturing(&self) -> bool { self.capture }
    pub fn output_pending(&self) -> bool { self.tx_size != 0 }
    pub fn request_pending(&self) -> bool { self.pending_ticket != 0 }
    pub fn background_work_ready(&self) -> bool {
        !self.capture && self.pending_ticket == 0 && self.tx_size == 0 && self.rx_size == 0
    }
    pub fn consumption_pending(&self, ticket: u32) -> bool {
        ticket != 0 && ticket == self.pending_ticket && self.consumed && !self.rendered
    }
    pub fn consumption_acknowledged(&self, ticket: u32) -> bool {
        self.consumption_pending(ticket) && self.notification == 2 && self.tx_size == 0
    }

    fn notifications(&mut self) {
        if self.pending_ticket == 0 || self.tx_size != 0 { return; }
        if self.notification == 1 && self.consumed {
            self.reply(self.pending_id, b"CONSUMED");
            self.notification = 2;
        } else if self.notification == 2 && self.rendered {
            let mut bytes = [0; 64];
            let mut body = Text::new(&mut bytes);
            let _ = write!(body, "{} gen={}",
                if self.refreshed { "RENDER_RETURN" } else { "NO_VISUAL_CHANGE" },
                self.completed_generation);
            self.reply(self.pending_id, body.written());
            self.pending_ticket = 0;
            self.notification = 0;
        }
    }

    /// One bounded bridge poll, between application loans. Before image
    /// confirmation, answer=false drains whole commands without claiming ready.
    /// build is the same immutable, boot-lived build ID on every invocation.
    pub fn poll(&mut self, channel: &mut dyn Channel, device: &mut dyn Device, build: &str, answer: bool) {
        let now = channel.now();
        if !channel.connected() {
            self.cancel();
            return;
        }
        if self.session != 0 && now.wrapping_sub(self.session_activity) >= 60_000 {
            self.cancel();
            return;
        }
        if self.capture && (now.wrapping_sub(self.lease_start) >= 15_000
            || now.wrapping_sub(self.lease_activity) >= 1_500) {
            self.capture = false;
            self.capture_expired = true;
        }
        if self.rx_size != 0 && now.wrapping_sub(self.rx_start) >= 1_000 {
            self.rx_size = 0;
            self.overflow = true;
        }
        if self.tx_size != 0 {
            let remaining = usize::from(self.tx_size - self.tx_offset);
            let sent = channel.write(&self.tx[usize::from(self.tx_offset)..usize::from(self.tx_size)]);
            if sent > remaining {
                self.cancel();
                return;
            }
            if sent != 0 {
                self.tx_offset += sent as u16;
                self.tx_progress = now;
            }
            if self.tx_offset == self.tx_size {
                self.tx_size = 0;
                self.tx_offset = 0;
            } else if now.wrapping_sub(self.tx_progress) >= 1_000 {
                self.cancel();
            }
            return;
        }
        self.notifications();
        if self.tx_size != 0 || self.pending_ticket != 0 {
            self.tx_progress = now;
            return;
        }
        for _ in 0..self.rx.len() {
            let next = channel.read_byte();
            if next < 0 { return; }
            if next == i32::from(b'\n') {
                if answer && self.overflow {
                    self.reply(0, b"ERROR line");
                } else if answer {
                    self.command(device, build, now);
                }
                self.rx_size = 0;
                self.overflow = false;
                self.tx_progress = now;
                return;
            }
            if self.rx_size == 0 && !self.overflow { self.rx_start = now; }
            if !(32..=126).contains(&next) || usize::from(self.rx_size) == self.rx.len() - 1 {
                self.overflow = true;
            }
            if !self.overflow {
                self.rx[usize::from(self.rx_size)] = next as u8;
                self.rx_size += 1;
            }
        }
    }
}

// Frozen Bridge is 668 B on RV32, including its 4 B build pointer. Lending the
// already-owned build string avoids that field; no replacement backing exists.
const _: () = assert!(core::mem::size_of::<Bridge>() == 664);
const _: () = assert!(core::mem::size_of::<Bridge>() + 32 * (core::mem::size_of::<ButtonEvent>() - 1) < 1024);
