// SPDX-License-Identifier: MIT
use super::*;

fn number(text: &str) -> Option<u32> {
    if text.is_empty() || (text.starts_with('0') && text.len() != 1) { return None; }
    text.bytes().try_fold(0u32, |value, byte| {
        if !byte.is_ascii_digit() { return None; }
        value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
    })
}
fn button(name: &str) -> Option<u8> {
    match name {
        "back" => Some(0), "activate" => Some(1), "previous" => Some(2),
        "next" => Some(3), "sleep" => Some(6), "purge-cache" => Some(PURGE_CACHE_BUTTON),
        _ => None,
    }
}

impl Bridge {
    pub(super) fn command(&mut self, device: &mut dyn Device, build: &str, now: u32) {
        // RX admits printable ASCII only; token references end before any reply
        // overwrites state. No second command buffer or unbounded parser exists.
        let Ok(rx) = core::str::from_utf8(&self.rx[..usize::from(self.rx_size)]) else {
            self.reply(0, b"ERROR line");
            return;
        };
        let mut tokens = [""; 8];
        let mut count = 0;
        for token in rx.split(' ') {
            if token.is_empty() || count == tokens.len() {
                // Frozen accepts an empty non-space line into its later syntax
                // check; it produces the same ERROR syntax here.
                self.reply(0, b"ERROR syntax");
                return;
            }
            tokens[count] = token;
            count += 1;
        }
        if count == 2 && tokens[0] == "N1" && tokens[1] == "HELLO" {
            self.hello(build, now);
            return;
        }
        let header = (number(tokens[1]), number(tokens[2]), number(tokens[3]));
        let (Some(boot), Some(session), Some(id)) = header else {
            self.reply(0, b"ERROR syntax");
            return;
        };
        if count < 5 || tokens[0] != "N1" || id == 0 {
            self.reply(0, b"ERROR syntax");
            return;
        }
        if self.session == 0 || boot != self.boot || session != self.session {
            self.reply(id, b"ERROR session");
            return;
        }
        if id <= self.last_id {
            self.reply(id, b"ERROR stale-id");
            return;
        }
        self.last_id = id;
        self.session_activity = now;
        if count == 5 && tokens[4] == "DIAG" {
            let diagnostics = device.diagnostics();
            self.diagnostics(id, diagnostics);
            return;
        }
        if count == 5 && tokens[4] == "PANEL" {
            self.panel(id, device.panel_ring());
            return;
        }
        if count == 5 && tokens[4] == "PUT" {
            // A granted lease sends its own first reply (newt_core::dev_put).
            self.capture = false;
            if let Err(busy) = device.put(newt_core::dev_put::Session { boot, session, request: id }) {
                self.reply(id, busy.reply());
            }
            return;
        }
        let status = device.status();
        if count == 5 && tokens[4] == "STATUS" {
            self.status_reply(id, status);
        } else if count == 6 && tokens[4] == "PRESS" {
            let code = button(tokens[5]);
            self.press(id, code, device);
        } else if count == 5 && tokens[4] == "FRAME" {
            self.frame_reply(id, status, device, now);
        } else if count == 7 && tokens[4] == "CHUNK" {
            let chunk = (number(tokens[5]), number(tokens[6]));
            self.chunk(id, chunk, status, device, now);
        } else {
            self.reply(id, b"ERROR command");
        }
    }

    fn hello(&mut self, build: &str, now: u32) {
        if self.session_counter == u32::MAX {
            self.reply(0, b"ERROR session-exhausted");
            return;
        }
        self.capture = false;
        self.pending_ticket = 0;
        self.notification = 0;
        self.session_counter += 1;
        self.session = self.session_counter;
        self.last_id = 0;
        self.session_activity = now;
        let mut bytes = [0; 160];
        let mut body = Text::new(&mut bytes);
        let build = build.split('\0').next().unwrap_or("");
        let _ = write!(body, "HELLO build={build} caps=press,status,frame,diag2,put wake=physical-only");
        self.reply(0, body.written());
    }

    /// A granted PUT's lease has ended: the session continues after its last
    /// ID, needs a fresh HELLO, or is answered as one.
    pub fn put_ended(&mut self, end: newt_core::dev_put::End, build: &str, now: u32) {
        match end {
            newt_core::dev_put::End::Kept { last_id } => {
                self.last_id = last_id;
                self.session_activity = now;
                self.tx_progress = now;
                self.tx_flush_pending = true;
            }
            newt_core::dev_put::End::Cancelled => {
                self.cancel();
                self.tx_progress = now;
                self.tx_flush_pending = true;
            }
            newt_core::dev_put::End::Hello => self.hello(build, now),
        }
    }
    /// A granted PUT whose lease could not start; the session continues.
    pub fn put_refused(&mut self, id: u32, body: &[u8]) { self.reply(id, body); }

    fn diagnostics(&mut self, id: u32, d: Diagnostics) {
        if !d.available {
            self.reply(id, b"DIAG version=2 available=0");
            return;
        }
        let mut bytes = [0; 464];
        let mut body = Text::new(&mut bytes);
        let _ = write!(body,
            "DIAG version=2 available=1 reset={} cause={} record={} requested={} restored={} checkpoint_saved={} book={} spine={} character={} reader_status={} reader_open={} progress_ready={} actual_spine={} actual_character={} first_input={} current_book={} heap={} largest={} opening={} library_count={} library_selected={} device={:016x}",
            d.reset, d.cause, d.record, d.requested, u8::from(d.restored),
            u8::from(d.checkpoint_saved), d.book, d.spine, d.character, d.reader_status,
            u8::from(d.reader_open), u8::from(d.progress_ready), d.actual_spine, d.actual_character,
            d.first_input, d.current_book, d.free_heap, d.largest_block, u8::from(d.opening),
            d.library_count, d.library_selected, d.device_fingerprint);
        self.reply(id, body.written());
    }

    fn panel(&mut self, id: u32, ring: Option<&PanelRing>) {
        let Some(ring) = ring else {
            self.reply(id, b"ERROR panel-unavailable");
            return;
        };
        let mut bytes = [0; 464];
        let mut body = Text::new(&mut bytes);
        let _ = write!(body, "PANEL version=1 capacity=31 next={} count={} data=", ring.next, ring.count);
        const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let size = usize::from(ring.count) * 8;
        if size == 0 { body.bytes(b"-"); }
        for i in (0..size).step_by(3) {
            let value = (u32::from(ring.byte(i)) << 16)
                | if i + 1 < size { u32::from(ring.byte(i + 1)) << 8 } else { 0 }
                | if i + 2 < size { u32::from(ring.byte(i + 2)) } else { 0 };
            body.bytes(&[
                ALPHABET[((value >> 18) & 63) as usize], ALPHABET[((value >> 12) & 63) as usize],
                if i + 1 < size { ALPHABET[((value >> 6) & 63) as usize] } else { b'=' },
                if i + 2 < size { ALPHABET[(value & 63) as usize] } else { b'=' },
            ]);
        }
        self.reply(id, body.written());
    }

    fn status_reply(&mut self, id: u32, status: Status) {
        let mut bytes = [0; 464];
        let mut body = Text::new(&mut bytes);
        let _ = write!(body, "STATUS screen={} gen={} ready={} refresh_ms={} heap={} dropped={} capture={}",
            status.screen, status.generation, u8::from(status.ready), status.refresh_ms,
            status.free_heap, status.dropped, u8::from(self.capture));
        self.reply(id, body.written());
    }

    fn press(&mut self, id: u32, code: Option<u8>, device: &mut dyn Device) {
        let Some(button) = code else { self.reply(id, b"ERROR button"); return; };
        self.capture = false;
        if self.ticket_counter == u32::MAX {
            self.reply(id, b"ERROR ticket-exhausted");
            return;
        }
        self.ticket_counter += 1;
        let ticket = self.ticket_counter;
        if !device.enqueue(ButtonEvent { ticket, button }) {
            self.reply(id, b"ERROR queue-full");
            return;
        }
        self.pending_ticket = ticket;
        self.pending_id = id;
        self.consumed = false;
        self.rendered = false;
        self.notification = 1;
        self.reply(id, b"ACCEPTED");
    }

    fn frame_reply(&mut self, id: u32, status: Status, device: &mut dyn Device, now: u32) {
        if self.capture_expired {
            self.reply(id, b"ERROR housekeeping-pending");
            return;
        }
        if !status.ready { self.reply(id, b"ERROR no-frame"); return; }
        let Some(frame) = device.frame() else { self.reply(id, b"ERROR no-frame"); return; };
        if !status.monochrome { self.reply(id, b"ERROR unsupported-format"); return; }
        if !self.capture {
            self.capture = true;
            self.lease_generation = status.generation;
            self.lease_start = now;
            self.lease_activity = now;
        }
        let checksum = crc32(frame);
        let mut bytes = [0; 464];
        let mut body = Text::new(&mut bytes);
        let _ = write!(body, "FRAME gen={} width={} height={} bytes={} format=pbm1 polarity=one-black orientation={} crc={checksum:08x} chunk={} refresh_ms={}",
            status.generation, status.width, status.height, FRAME_BYTES,
            status.orientation.name(), CHUNK_BYTES, status.refresh_ms);
        self.reply(id, body.written());
    }

    fn chunk(&mut self, id: u32, chunk: (Option<u32>, Option<u32>), status: Status, device: &mut dyn Device, now: u32) {
        let (Some(generation), Some(offset)) = chunk else { self.reply(id, b"ERROR chunk"); return; };
        let offset = offset as usize;
        if offset >= FRAME_BYTES || offset % CHUNK_BYTES != 0 { self.reply(id, b"ERROR chunk"); return; }
        if !self.capture || generation != self.lease_generation || status.generation != self.lease_generation {
            self.capture = false;
            self.reply(id, b"ERROR capture-expired");
            return;
        }
        if !status.monochrome || !status.ready {
            self.capture = false;
            self.reply(id, b"ERROR unsupported-frame");
            return;
        }
        let Some(frame) = device.frame() else {
            self.capture = false;
            self.reply(id, b"ERROR unsupported-frame");
            return;
        };
        self.lease_activity = now;
        let size = (FRAME_BYTES - offset).min(CHUNK_BYTES);
        let mut bytes = [0; 464];
        let mut body = Text::new(&mut bytes);
        let _ = write!(body, "CHUNK {generation} {offset} ");
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in &frame[offset..offset + size] {
            body.bytes(&[HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]]);
        }
        self.reply(id, body.written());
        if offset + size == FRAME_BYTES { self.capture = false; }
    }
}
