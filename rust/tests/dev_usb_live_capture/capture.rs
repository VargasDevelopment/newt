// SPDX-License-Identifier: MIT
//! Host regression for #38 (NEWT-172): full 52,272-byte DEV USB live capture
//! over ESP32-C3 HWCDC's 256-byte wrapping TX ring buffer, 64-byte EP1 IN FIFO
//! auto-flush, and Linux cdc_acm's 512-byte Bulk IN URB.
//!
//! Compiles the real firmware bridge (`rust/firmware/dev_usb.rs`) and real
//! board channel (`rust/salamander/board/dev_usb.rs`) against a packet- and
//! state-level HWCDC + USB_SERIAL_JTAG + cdc_acm transport model. Because this
//! host harness swaps out the native C++ `nsc_*` helpers
//! (`rust/firmware/serial_console/native/serial_console.cpp`), breaking the
//! real C++ register flush would not fail this test; real-flush evidence comes
//! from Virtual X3 and bench hardware runs.

use std::cell::RefCell;
use std::collections::VecDeque;

#[allow(dead_code, unused_imports)]
#[path = "../../firmware/dev_usb.rs"]
mod dev_usb;

#[allow(dead_code)]
#[path = "../../salamander/board/dev_usb.rs"]
mod board_dev_usb;

use dev_usb::{Bridge, ButtonEvent, Channel, Device, Diagnostics, PanelRing, Status, CHUNK_BYTES, FRAME_BYTES};
use newt_core::wire::crc32;

const TX_RING_CAPACITY: usize = 256;
const EP1_MAX_PACKET: usize = 64;
const CDC_ACM_URB_BYTES: usize = 512;

/// Faithful model of ESP32-C3 USB_SERIAL_JTAG + Arduino HWCDC + Linux cdc_acm.
struct HwCdcModel {
    connected: bool,
    rx_queue: VecDeque<u8>,
    tx_ring: [u8; TX_RING_CAPACITY],
    tx_read: usize,
    tx_write: usize,
    tx_len: usize,
    hw_fifo: Vec<u8>,
    hw_fifo_busy: bool,
    in_empty_enabled: bool,
    host_drain_paused: bool,
    host_urb: Vec<u8>,
    host_delivered: Vec<u8>,
    zlp_count: usize,
}

impl HwCdcModel {
    fn new(start_offset: usize) -> Self {
        let pos = start_offset % TX_RING_CAPACITY;
        Self {
            connected: true,
            rx_queue: VecDeque::new(),
            tx_ring: [0; TX_RING_CAPACITY],
            tx_read: pos,
            tx_write: pos,
            tx_len: 0,
            hw_fifo: Vec::new(),
            hw_fifo_busy: false,
            in_empty_enabled: false,
            host_drain_paused: false,
            host_urb: Vec::new(),
            host_delivered: Vec::new(),
            zlp_count: 0,
        }
    }

    fn available_for_write(&self) -> i32 {
        (TX_RING_CAPACITY - self.tx_len) as i32
    }

    fn txfifo_writable(&self) -> bool {
        !self.hw_fifo_busy
    }

    /// Mirrors `usb_serial_jtag_ll_txfifo_flush()` (`usb_serial_jtag_ll.h:168-182`):
    /// a no-op if a packet is already staged/auto-flushed; otherwise stages the
    /// current FIFO contents (0 bytes = ZLP, 1..63 bytes = short packet).
    fn txfifo_flush(&mut self) {
        if self.hw_fifo_busy {
            return;
        }
        self.hw_fifo_busy = true;
    }

    /// Mirrors `hw_cdc_isr_handler()` (`HWCDC.cpp:96-118`):
    /// pops one contiguous slice (up to 64 bytes, stopping at ring wrap) from
    /// `tx_ring_buf`, writes it to the 64-byte EP1 IN FIFO, flushes, and
    /// re-enables `IN_EMPTY`. When `tx_ring_buf` is empty it disables `IN_EMPTY`
    /// without sending a ZLP.
    fn run_isr(&mut self) {
        if !self.in_empty_enabled || !self.txfifo_writable() {
            return;
        }
        self.in_empty_enabled = false;
        if self.tx_len == 0 {
            return;
        }
        let contiguous = self.tx_len.min(TX_RING_CAPACITY - self.tx_read);
        let take = contiguous.min(EP1_MAX_PACKET);
        self.hw_fifo.clear();
        self.hw_fifo.extend_from_slice(&self.tx_ring[self.tx_read..self.tx_read + take]);
        self.tx_read = (self.tx_read + take) % TX_RING_CAPACITY;
        self.tx_len -= take;
        // Hardware auto-flushes a full 64-byte packet so the ISR's immediate
        // txfifo_flush() is a no-op; short packets are flushed by txfifo_flush().
        self.hw_fifo_busy = true;
        if self.connected {
            self.in_empty_enabled = true;
        }
    }

    /// Mirrors `HWCDC::write()` (`HWCDC.cpp:426-498`) for a single nonblocking
    /// write bounded by `available_for_write()`.
    fn write(&mut self, bytes: &[u8]) -> usize {
        if !self.connected || bytes.is_empty() {
            return 0;
        }
        let room = TX_RING_CAPACITY - self.tx_len;
        let count = bytes.len().min(room);
        for &b in &bytes[..count] {
            self.tx_ring[self.tx_write] = b;
            self.tx_write = (self.tx_write + 1) % TX_RING_CAPACITY;
            self.tx_len += 1;
        }
        if count > 0 {
            self.in_empty_enabled = true;
            // On ESP32-C3, IN_EMPTY raw status is asserted whenever txfifo is
            // writable, so enabling the interrupt vectors into the ISR at once.
            self.run_isr();
        }
        count
    }

    /// Mirrors `nsc_wake_transmitter()` (`serial_console.cpp:31-37`).
    fn wake_transmitter(&mut self) -> u8 {
        let writable = if self.txfifo_writable() { 1 } else { 0 };
        self.txfifo_flush();
        self.in_empty_enabled = true;
        // If a packet (or ZLP) was just staged, hw_fifo_busy is now true, so
        // the ISR runs only after the host collects it.
        self.run_isr();
        writable
    }

    /// Simulates one Full-Speed USB IN poll (~1 ms) by Linux `cdc_acm`.
    fn host_collect_one_packet(&mut self) -> bool {
        if self.host_drain_paused || !self.hw_fifo_busy {
            return false;
        }
        let pkt = std::mem::take(&mut self.hw_fifo);
        self.hw_fifo_busy = false;
        let len = pkt.len();
        if len == 0 {
            self.zlp_count += 1;
        }
        self.host_urb.extend_from_slice(&pkt);
        // Linux cdc_acm completes its 512-byte Bulk IN URB only on a short/zero
        // packet (< 64 bytes) or when the URB fills to 512 bytes.
        if len < EP1_MAX_PACKET || self.host_urb.len() >= CDC_ACM_URB_BYTES {
            self.host_delivered.extend_from_slice(&self.host_urb);
            self.host_urb.clear();
        }
        // Host collected the FIFO -> IN_EMPTY fires.
        self.run_isr();
        true
    }

    /// Simulates up to `frames` 1 ms Full-Speed USB IN polls.
    fn step_usb_frames(&mut self, frames: usize) {
        for _ in 0..frames {
            if !self.host_collect_one_packet() {
                break;
            }
        }
    }

    fn host_send_line(&mut self, line: &str) {
        for &b in line.as_bytes() {
            self.rx_queue.push_back(b);
        }
        self.rx_queue.push_back(b'\n');
    }

    fn take_delivered_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(pos) = self.host_delivered.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8(self.host_delivered[..pos].to_vec()).expect("utf8 line");
            self.host_delivered.drain(..=pos);
            lines.push(line);
        }
        lines
    }
}

thread_local! {
    static MODEL: RefCell<HwCdcModel> = RefCell::new(HwCdcModel::new(0));
}

fn with_model<R>(f: impl FnOnce(&mut HwCdcModel) -> R) -> R {
    MODEL.with(|cell| f(&mut cell.borrow_mut()))
}

#[unsafe(no_mangle)]
pub extern "C" fn nsc_connected() -> u8 {
    with_model(|m| u8::from(m.connected))
}

#[unsafe(no_mangle)]
pub extern "C" fn nsc_available() -> i32 {
    with_model(|m| m.rx_queue.len() as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn nsc_read_byte() -> i32 {
    with_model(|m| m.rx_queue.pop_front().map_or(-1, i32::from))
}

#[unsafe(no_mangle)]
pub extern "C" fn nsc_available_for_write() -> i32 {
    with_model(|m| m.available_for_write())
}

#[unsafe(no_mangle)]
pub extern "C" fn nsc_wake_transmitter() -> u8 {
    with_model(|m| m.wake_transmitter())
}

#[unsafe(no_mangle)]
pub extern "C" fn nsc_write(data: *const u8, len: usize) -> usize {
    if data.is_null() || len == 0 {
        return 0;
    }
    // SAFETY: `board_dev_usb::write` passes a valid slice pointer and length.
    let slice = unsafe { std::slice::from_raw_parts(data, len) };
    with_model(|m| m.write(slice))
}

#[unsafe(no_mangle)]
pub extern "C" fn nsc_flush() {}

/// Bridges the real `dev_usb::Channel` trait straight to `board_dev_usb`.
struct BoardChannel {
    now_ms: u32,
}

impl Channel for BoardChannel {
    fn connected(&mut self) -> bool { board_dev_usb::connected() }
    fn read_byte(&mut self) -> i32 { board_dev_usb::read_byte() }
    fn write(&mut self, bytes: &[u8]) -> usize { board_dev_usb::write(bytes) }
    fn flush_tx_tail(&mut self) -> bool { board_dev_usb::flush_tx_tail() }
    fn now(&mut self) -> u32 { self.now_ms }
}

struct FakeDevice {
    status: Status,
    frame: Box<[u8; FRAME_BYTES]>,
    queued: Vec<ButtonEvent>,
    accept_put: bool,
    granted_put: Option<newt_core::dev_put::Session>,
}

impl FakeDevice {
    fn new(screen: u32, generation: u32) -> Self {
        let mut frame = vec![0u8; FRAME_BYTES].into_boxed_slice();
        for (i, byte) in frame.iter_mut().enumerate() {
            *byte = ((i * 37 + i / 251 + 19) & 0xff) as u8;
        }
        Self {
            status: Status {
                generation,
                refresh_ms: 120,
                free_heap: 140_000,
                dropped: 0,
                screen,
                width: 528,
                height: 792,
                ready: true,
                orientation: newt_core::preferences::Orientation::Portrait,
                monochrome: true,
            },
            frame: frame.try_into().expect("exact frame size"),
            queued: Vec::new(),
            accept_put: false,
            granted_put: None,
        }
    }
}

impl Device for FakeDevice {
    fn status(&mut self) -> Status { self.status }
    fn frame(&mut self) -> Option<&[u8; FRAME_BYTES]> { Some(&self.frame) }
    fn enqueue(&mut self, event: ButtonEvent) -> bool { self.queued.push(event); true }
    fn panel_ring(&mut self) -> Option<&PanelRing> { None }
    fn diagnostics(&mut self) -> Diagnostics { Diagnostics::default() }
    fn put(&mut self, session: newt_core::dev_put::Session) -> Result<(), newt_core::dev_put::Busy> {
        if self.accept_put {
            self.granted_put = Some(session);
            Ok(())
        } else {
            Err(newt_core::dev_put::Busy::NotHome)
        }
    }
}

fn parse_protocol_line(line: &str, expected_boot: u32, expected_session: u32, expected_id: u32) -> String {
    let (payload, crc_hex) = line.rsplit_once(" *").expect("crc delimiter");
    assert_eq!(crc_hex.len(), 8, "crc width in {line}");
    let got_crc = u32::from_str_radix(crc_hex, 16).expect("crc hex");
    assert_eq!(got_crc, crc32(payload.as_bytes()), "crc mismatch in {line}");
    let mut parts = payload.splitn(5, ' ');
    assert_eq!(parts.next(), Some("!N1"));
    assert_eq!(parts.next().unwrap().parse::<u32>().unwrap(), expected_boot);
    assert_eq!(parts.next().unwrap().parse::<u32>().unwrap(), expected_session);
    assert_eq!(parts.next().unwrap().parse::<u32>().unwrap(), expected_id);
    parts.next().unwrap().to_string()
}

/// Runs one request/reply exchange at the firmware's real 10 ms poll cadence.
/// When `idle_log` is Some(text), simulates Settings' idle Serial log once
/// `bridge.background_work_ready()` becomes true after capture lease expiry.
fn exchange(
    bridge: &mut Bridge,
    channel: &mut BoardChannel,
    device: &mut FakeDevice,
    command: &str,
    boot: u32,
    session: u32,
    id: u32,
    idle_log: Option<&[u8]>,
) -> String {
    with_model(|m| m.host_send_line(command));
    let deadline_ms = channel.now_ms + 3_000;
    let mut idle_logged_after_expiry = false;
    while channel.now_ms < deadline_ms {
        bridge.poll(channel, device, "8dad3b493384", true);
        // Mirror run_loop.rs:226-227: once background_work_ready() is true
        // (e.g. after 1,500 ms capture lease expiry on Settings), idle work
        // logs a Serial line, which flushes any stalled 64-byte USB URB.
        if let Some(log_bytes) = idle_log {
            if !idle_logged_after_expiry && !bridge.capturing() && bridge.background_work_ready() && id > 2 {
                with_model(|m| { let _ = m.write(log_bytes); });
                idle_logged_after_expiry = true;
            }
        }
        with_model(|m| m.step_usb_frames(10));
        let lines = with_model(|m| m.take_delivered_lines());
        for line in lines {
            if line.starts_with("!N1 ") {
                return parse_protocol_line(&line, boot, session, id);
            }
        }
        channel.now_ms += 10;
    }
    panic!("Protocol response timed out for `{command}` at t={}ms (urb_len={}, tx_ring_len={})",
        channel.now_ms,
        with_model(|m| m.host_urb.len()),
        with_model(|m| m.tx_len));
}

fn run_full_capture(start_offset: usize, boot: u32, generation: u32, screen: u32, idle_log: Option<&[u8]>) {
    with_model(|m| *m = HwCdcModel::new(start_offset));
    let mut bridge = Bridge::new(boot);
    let mut channel = BoardChannel { now_ms: 1_000 };
    let mut device = FakeDevice::new(screen, generation);

    let hello = exchange(&mut bridge, &mut channel, &mut device, "N1 HELLO", boot, 1, 0, None);
    assert!(hello.starts_with("HELLO build=8dad3b493384 "), "unexpected hello: {hello}");

    let mut req_id = 0x8000_0001u32;
    let frame_hdr = exchange(
        &mut bridge, &mut channel, &mut device,
        &format!("N1 {boot} 1 {req_id} FRAME"), boot, 1, req_id, None,
    );
    let expected_crc = format!("crc={:08x}", crc32(&*device.frame));
    assert!(frame_hdr.contains(&expected_crc), "missing frame crc in {frame_hdr}");

    let mut reconstructed = Vec::with_capacity(FRAME_BYTES);
    let mut offset = 0usize;
    while offset < FRAME_BYTES {
        req_id += 1;
        let body = exchange(
            &mut bridge, &mut channel, &mut device,
            &format!("N1 {boot} 1 {req_id} CHUNK {generation} {offset}"),
            boot, 1, req_id, idle_log,
        );
        assert!(
            !body.starts_with("ERROR "),
            "capture failed at offset={offset} start_offset={start_offset}: {body}"
        );
        let prefix = format!("CHUNK {generation} {offset} ");
        let hex = body.strip_prefix(&prefix).unwrap_or_else(|| panic!("bad chunk header: {body}"));
        let expected_len = (FRAME_BYTES - offset).min(CHUNK_BYTES);
        assert_eq!(hex.len(), expected_len * 2);
        for pair in hex.as_bytes().chunks_exact(2) {
            let s = std::str::from_utf8(pair).unwrap();
            reconstructed.push(u8::from_str_radix(s, 16).unwrap());
        }
        offset += expected_len;
    }

    assert_eq!(reconstructed.len(), FRAME_BYTES);
    assert_eq!(crc32(&reconstructed), crc32(&*device.frame), "whole-frame CRC mismatch");
    assert_eq!(reconstructed.as_slice(), device.frame.as_slice());
}

#[test]
fn apps_live_capture_completes_across_all_256_ring_offsets() {
    // 10-digit boot + 10-digit req_id yields WL5 = 373 (odd; gcd(117, 256) = 1),
    // which hits wrap-to-64 within every 327-chunk capture on Apps (screen=4,
    // no idle Serial logs).
    for start_offset in 0..TX_RING_CAPACITY {
        run_full_capture(start_offset, 3_622_628_779, 3, 4, None);
    }
    // Exercise remaining board_dev_usb helpers.
    assert_eq!(board_dev_usb::available(), 0);
    board_dev_usb::flush();
}

#[test]
fn settings_live_capture_never_expires_under_idle_serial_log() {
    // On Settings (screen=3), an idle STATS log fires once background_work_ready()
    // becomes true after the 1,500 ms capture lease expires. Without flush_tx_tail,
    // that log unsticks the stalled 64-byte chunk and the next CHUNK gets
    // `ERROR capture-expired`.
    let stats_log = b"STATS: live capture idle tick\n";
    for &(boot, generation, start_offset) in &[
        (3_622_628_779u32, 3u32, 0usize),   // 10-digit boot (WL5=373, odd)
        (3_622_628_779, 3, 139),            // hits offset 64 early in capture
        (241_508_120, 3, 0),                // 9-digit boot (WL5=372, even)
        (241_508_120, 3, 204),              // even offset landing on 64
        (3_622_628_779, 10, 64),            // 2-digit generation (WL5=374, even)
    ] {
        run_full_capture(start_offset, boot, generation, 3, Some(stats_log));
    }
}

#[test]
fn flush_tx_tail_waits_for_ring_drain_and_hw_fifo_before_sending_zlp() {
    // Construct a reply that wraps in tx_ring_buf and ends at offset 64.
    // STATUS reply with boot=100, session=1, id=1 is 86 bytes:
    //   "!N1 100 1 1 STATUS screen=3 gen=3 ready=1 refresh_ms=120 heap=140000 dropped=0 capture=0 *XXXXXXXX\n"
    // Start at ring offset (256 + 64 - 99) = 221 so the post-wrap tail is
    // 64 bytes once we check exact reply length dynamically.
    let mut probe_bridge = Bridge::new(100);
    let mut probe_ch = BoardChannel { now_ms: 1_000 };
    let mut dev = FakeDevice::new(3, 3);
    with_model(|m| *m = HwCdcModel::new(0));
    let _ = exchange(&mut probe_bridge, &mut probe_ch, &mut dev, "N1 HELLO", 100, 1, 0, None);
    with_model(|m| { m.host_delivered.clear(); m.host_urb.clear(); });
    let status_body = exchange(&mut probe_bridge, &mut probe_ch, &mut dev, "N1 100 1 1 STATUS", 100, 1, 1, None);
    let reply_len = format!("!N1 100 1 1 {status_body} *00000000\n").len();
    assert!(reply_len > EP1_MAX_PACKET && reply_len < TX_RING_CAPACITY);
    let start_pos = (TX_RING_CAPACITY + EP1_MAX_PACKET - reply_len) % TX_RING_CAPACITY;

    // Now replay STATUS starting at `start_pos` with host USB draining paused
    // step-by-step so Poll 4 sees ring non-empty, Poll 5 sees ring empty + HW
    // FIFO still holding the 64-byte packet (txfifo_writable() == 0), and
    // Poll 6 sees HW FIFO writable (txfifo_writable() == 1) -> sends ZLP.
    with_model(|m| *m = HwCdcModel::new(0));
    let mut bridge = Bridge::new(100);
    let mut ch = BoardChannel { now_ms: 1_000 };
    let _ = exchange(&mut bridge, &mut ch, &mut dev, "N1 HELLO", 100, 1, 0, None);

    // Reposition empty ring to `start_pos` and pause host draining.
    with_model(|m| {
        m.tx_read = start_pos;
        m.tx_write = start_pos;
        m.tx_len = 0;
        m.host_urb.clear();
        m.host_delivered.clear();
        m.zlp_count = 0;
        m.host_drain_paused = true;
        m.host_send_line("N1 100 1 1 STATUS");
    });

    // Poll 1: reads STATUS command and stages reply in Bridge.tx.
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(bridge.output_pending());
    ch.now_ms += 10;

    // Poll 2: writes all `reply_len` bytes into tx_ring_buf. Because
    // board_dev_usb::write() wakes the transmitter before nsc_write(), a
    // pre-write ZLP is staged in hw_fifo while host_drain_paused == true,
    // holding all `reply_len` bytes in tx_ring_buf.
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(bridge.output_pending(), "tx_flush_pending must remain set after write");
    assert!(!bridge.background_work_ready(), "idle work must wait for tail flush");
    assert_eq!(with_model(|m| m.tx_len), reply_len);
    assert!(with_model(|m| m.hw_fifo_busy));
    ch.now_ms += 10;

    // Poll 3 (ring still has `reply_len` bytes!): flush_tx_tail() MUST return false.
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(bridge.output_pending(), "must not clear flush while tx_ring_buf is non-empty");
    assert_eq!(with_model(|m| m.zlp_count), 0);

    // Host collects the pre-write ZLP -> ISR moves the pre-wrap short packet
    // into hw_fifo, leaving exactly 64 bytes in tx_ring_buf.
    with_model(|m| {
        m.host_drain_paused = false;
        assert!(m.host_collect_one_packet());
        m.host_drain_paused = true;
    });
    assert_eq!(with_model(|m| m.tx_len), EP1_MAX_PACKET);
    assert_eq!(with_model(|m| m.hw_fifo.len()), reply_len - EP1_MAX_PACKET);
    assert!(with_model(|m| m.hw_fifo_busy));
    ch.now_ms += 10;

    // Poll 4 (ring still has 64 bytes, and even if HW FIFO is momentarily
    // writable with IN_EMPTY paused before the ISR pops those 64 bytes):
    // flush_tx_tail() MUST wake the transmitter and still return false!
    with_model(|m| {
        m.host_drain_paused = false;
        // Host collects the pre-wrap short packet while IN_EMPTY is masked,
        // leaving hw_fifo writable (empty) and 64 bytes still in tx_ring_buf.
        m.in_empty_enabled = false;
        assert!(m.host_collect_one_packet());
        assert_eq!(m.tx_len, EP1_MAX_PACKET);
        assert!(!m.hw_fifo_busy);
        m.host_drain_paused = true;
    });
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(
        bridge.output_pending(),
        "must not clear flush while 64-byte tail is still queued in tx_ring_buf even when HW FIFO is writable"
    );
    // The wake staged a ZLP and re-enabled IN_EMPTY; once host collects that
    // ZLP, the ISR moves the 64-byte tail into hw_fifo (auto-flushed, busy).
    with_model(|m| {
        m.host_drain_paused = false;
        assert!(m.host_collect_one_packet());
        m.host_drain_paused = true;
    });
    assert_eq!(with_model(|m| m.tx_len), 0);
    assert_eq!(with_model(|m| m.hw_fifo.len()), EP1_MAX_PACKET);
    assert!(with_model(|m| m.hw_fifo_busy));
    ch.now_ms += 10;

    // Poll 5 (ring empty, but 64-byte packet STILL in HW FIFO -> txfifo_writable == 0!):
    // flush_tx_tail() MUST return false and keep retrying.
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(bridge.output_pending(), "must not clear flush while 64-byte packet is still in HW FIFO");

    // Host collects the 64-byte packet -> URB is now STUCK holding 64 bytes,
    // and HW FIFO is empty (txfifo_writable() == 1).
    with_model(|m| {
        m.host_drain_paused = false;
        assert!(m.host_collect_one_packet());
        assert_eq!(m.host_urb.len(), EP1_MAX_PACKET);
        assert!(m.take_delivered_lines().is_empty(), "64-byte packet must stay held in cdc_acm URB");
        m.host_drain_paused = true;
    });
    let zlp_before_tail_flush = with_model(|m| m.zlp_count);
    ch.now_ms += 10;

    // Poll 6: ring empty AND txfifo_writable() == 1 -> sends ZLP and clears pending!
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(!bridge.output_pending(), "flush must clear once writable HW FIFO accepts ZLP");
    assert!(bridge.background_work_ready());

    // Host collects the ZLP -> delivers the complete STATUS line!
    with_model(|m| {
        m.host_drain_paused = false;
        assert!(m.host_collect_one_packet());
    });
    assert_eq!(with_model(|m| m.zlp_count), zlp_before_tail_flush + 1);
    let delivered = with_model(|m| m.take_delivered_lines());
    assert_eq!(delivered.len(), 1);
    let body = parse_protocol_line(&delivered[0], 100, 1, 1);
    assert_eq!(body, status_body);
}

#[test]
fn stalled_host_cancels_flush_and_consumed_waits_for_tail_flush() {
    // Part A: if the host stops draining USB while tx_flush_pending is true,
    // the 1,000 ms tx_progress timeout cancels cleanly instead of blocking
    // background_work_ready() indefinitely.
    with_model(|m| *m = HwCdcModel::new(0));
    let mut bridge = Bridge::new(100);
    let mut ch = BoardChannel { now_ms: 1_000 };
    let mut dev = FakeDevice::new(3, 3);
    with_model(|m| {
        m.host_drain_paused = true;
        m.host_send_line("N1 HELLO");
    });
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true); // reads HELLO
    ch.now_ms += 10;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true); // writes HELLO -> tx_flush_pending=true
    assert!(bridge.output_pending());
    assert!(!bridge.background_work_ready());
    ch.now_ms += 990;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true); // t=2000, elapsed 990 < 1000
    assert!(bridge.output_pending());
    ch.now_ms += 10;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true); // elapsed 1000 >= 1000 -> cancel
    assert!(!bridge.output_pending());
    assert!(bridge.background_work_ready());

    // Part B: CONSUMED acknowledgement waits until its tail flush finishes.
    with_model(|m| *m = HwCdcModel::new(0));
    let mut bridge = Bridge::new(100);
    let mut ch = BoardChannel { now_ms: 1_000 };
    let _ = exchange(&mut bridge, &mut ch, &mut dev, "N1 HELLO", 100, 1, 0, None);
    let acc = exchange(&mut bridge, &mut ch, &mut dev, "N1 100 1 1 PRESS next", 100, 1, 1, None);
    assert_eq!(acc, "ACCEPTED");
    assert_eq!(dev.queued.len(), 1);
    let ticket = dev.queued[0].ticket;
    assert!(bridge.consume(ticket, true, ch.now_ms));

    // Pause host USB draining while CONSUMED is written.
    with_model(|m| m.host_drain_paused = true);
    ch.now_ms += 1;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true); // notifications() queues CONSUMED
    ch.now_ms += 1;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true); // writes CONSUMED -> tx_flush_pending=true
    assert!(!bridge.consumption_acknowledged(ticket), "must wait for tail flush");

    // Resume USB draining -> next poll flushes ZLP and acknowledges consumption.
    with_model(|m| {
        m.host_drain_paused = false;
        m.step_usb_frames(10);
    });
    ch.now_ms += 1;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(bridge.consumption_acknowledged(ticket));
}

struct NoopCard;
impl newt_core::dev_put::Card for NoopCard {
    fn entry(&mut self, _: &core::ffi::CStr) -> Result<newt_core::dev_put::Entry, i32> { Err(-1) }
    fn make_folder(&mut self, _: &core::ffi::CStr) -> Result<(), i32> { Err(-1) }
    fn create(&mut self, _: &core::ffi::CStr) -> Result<i32, i32> { Err(-1) }
    fn open_read(&mut self, _: &core::ffi::CStr) -> Result<i32, i32> { Err(-1) }
    fn write(&mut self, _: i32, _: &[u8]) -> Result<usize, i32> { Err(-1) }
    fn read(&mut self, _: i32, _: &mut [u8]) -> Result<usize, i32> { Err(-1) }
    fn sync(&mut self, _: i32) -> Result<(), i32> { Err(-1) }
    fn close(&mut self, _: i32) -> Result<(), i32> { Err(-1) }
    fn remove(&mut self, _: &core::ffi::CStr) -> Result<(), i32> { Err(-1) }
    fn rename(&mut self, _: &core::ffi::CStr, _: &core::ffi::CStr) -> Result<(), i32> { Err(-1) }
    fn free_bytes(&mut self) -> Result<u64, i32> { Err(-1) }
}

struct NoopHash;
impl newt_core::update_online::Hash for NoopHash {
    fn start(&mut self) -> bool { true }
    fn update(&mut self, _: &[u8]) -> bool { true }
    fn finish(&mut self, _: &mut [u8; 32]) -> bool { true }
}

/// Mirrors `rust/firmware/app_runtime/card_write.rs::LeaseWire`: tracks
/// `tx_flush_pending` only for the lease lifetime and drops it on return.
struct TestLeaseWire {
    now_ms: u32,
    tx_flush_pending: bool,
    injected_bad_line: bool,
    pause_host_on_abandon: bool,
}

impl newt_core::dev_put::Wire for TestLeaseWire {
    fn connected(&mut self) -> bool { board_dev_usb::connected() }
    fn read_byte(&mut self) -> i32 {
        let byte = board_dev_usb::read_byte();
        if byte < 0 && self.tx_flush_pending && board_dev_usb::flush_tx_tail() {
            self.tx_flush_pending = false;
        }
        byte
    }
    fn write(&mut self, bytes: &[u8]) -> usize {
        if self.injected_bad_line && self.pause_host_on_abandon {
            with_model(|m| m.host_drain_paused = true);
        }
        let sent = board_dev_usb::write(bytes);
        if sent != 0 {
            self.tx_flush_pending = true;
        }
        sent
    }
    fn now(&mut self) -> u32 { self.now_ms }
    fn physical_input(&mut self) -> bool { false }
    fn pause(&mut self) {
        self.now_ms += 1;
        with_model(|m| m.step_usb_frames(1));
        if !self.injected_bad_line {
            let lines = with_model(|m| m.take_delivered_lines());
            if lines.iter().any(|l| l.contains(" PUT ")) {
                // Reposition empty TX ring to offset 252 so the 68-byte
                // `ERROR abandoned reason=syntax` reply splits as 4 + 64 bytes
                // across the 256-byte ring wrap.
                with_model(|m| {
                    assert_eq!(m.tx_len, 0);
                    assert!(!m.hw_fifo_busy);
                    m.tx_read = 252;
                    m.tx_write = 252;
                    m.host_urb.clear();
                    m.host_delivered.clear();
                    m.host_send_line("bad syntax");
                });
                self.injected_bad_line = true;
            }
        }
    }
}

#[test]
fn cancelled_put_reply_flushes_64_byte_tail() {
    // Concrete 68-byte syntax-abandon reply:
    // `!N1 3622628779 1 2147483649 ERROR abandoned reason=syntax *XXXXXXXX\n`
    // Starting at ring offset 252, HWCDC sends 4 bytes (252..256) then a full
    // 64-byte packet (0..64) that Linux cdc_acm holds in its 512-byte URB
    // unless Bridge flushes a ZLP after `put_ended(End::Cancelled, ...)`.
    const BOOT: u32 = 3_622_628_779;
    const REQ_ID: u32 = 2_147_483_649;

    with_model(|m| *m = HwCdcModel::new(0));
    let mut bridge = Bridge::new(BOOT);
    let mut ch = BoardChannel { now_ms: 1_000 };
    let mut dev = FakeDevice::new(0, 1);
    dev.accept_put = true;

    let _ = exchange(&mut bridge, &mut ch, &mut dev, "N1 HELLO", BOOT, 1, 0, None);
    // Let HELLO's own tail flush complete before PUT arrives, matching steady
    // idle state on Home so `bridge.tx_flush_pending` starts strictly false.
    ch.now_ms += 10;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(!bridge.output_pending(), "HELLO tail flush must finish before PUT");
    with_model(|m| {
        m.step_usb_frames(2);
        m.host_delivered.clear();
        m.host_urb.clear();
        m.host_send_line(&format!("N1 {BOOT} 1 {REQ_ID} PUT"));
    });
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    let session = dev.granted_put.take().expect("PUT lease granted");
    assert!(!bridge.output_pending(), "PUT admission leaves Bridge flush idle");

    // Run the real `newt_core::dev_put::run()` until it sends the 68-byte
    // `ERROR abandoned reason=syntax` reply and returns `End::Cancelled`.
    let mut memory = [0u8; newt_core::dev_put::MEMORY_BYTES];
    let mut card = NoopCard;
    let mut hash = NoopHash;
    let end = {
        let mut wire = TestLeaseWire {
            now_ms: ch.now_ms,
            tx_flush_pending: false,
            injected_bad_line: false,
            pause_host_on_abandon: true,
        };
        let end = newt_core::dev_put::run(session, &mut memory, &mut wire, &mut card, &mut hash);
        ch.now_ms = wire.now_ms;
        assert!(wire.injected_bad_line);
        assert!(wire.tx_flush_pending, "lease dropped with pending tail flush");
        end
    };
    assert_eq!(end, newt_core::dev_put::End::Cancelled);

    bridge.put_ended(end, "8dad3b493384", ch.now_ms);
    // Session cancellation (e.g. a subsequent physical button press) must keep
    // the pending transport tail flush alive.
    bridge.physical_input();
    assert!(bridge.output_pending(), "cancelled PUT must keep tail flush pending");

    // Host drains the pre-write ZLP, the 4-byte pre-wrap short packet (252..256),
    // and the 64-byte post-wrap packet (0..64). The 64-byte tail stays held in
    // cdc_acm's URB with no later serial output.
    with_model(|m| {
        m.host_drain_paused = false;
        m.step_usb_frames(10);
        assert_eq!(m.tx_len, 0);
        assert!(!m.hw_fifo_busy);
        assert_eq!(m.host_urb.len(), EP1_MAX_PACKET, "last 64 bytes must wait in URB for ZLP");
        assert!(m.take_delivered_lines().is_empty());
    });

    // Polling Bridge with zero subsequent serial output sends the ZLP and
    // delivers the full 68-byte syntax-abandon reply.
    ch.now_ms += 10;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(!bridge.output_pending(), "tail flush must clear once ZLP is staged");
    assert!(bridge.background_work_ready());
    with_model(|m| m.step_usb_frames(2));
    let delivered = with_model(|m| m.take_delivered_lines());
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].len() + 1, 68);
    assert_eq!(
        parse_protocol_line(&delivered[0], BOOT, 1, REQ_ID),
        "ERROR abandoned reason=syntax"
    );

    // The protocol session itself was cancelled, so reusing session 1 fails.
    let err = exchange(&mut bridge, &mut ch, &mut dev, &format!("N1 {BOOT} 1 {} STATUS", REQ_ID + 1), BOOT, 0, REQ_ID + 1, None);
    assert_eq!(err, "ERROR session");

    // Bounded timeout: if the host stops draining USB after a cancelled PUT,
    // the 1,000 ms tx_progress limit clears `tx_flush_pending` cleanly.
    with_model(|m| m.host_drain_paused = true);
    bridge.put_ended(newt_core::dev_put::End::Cancelled, "8dad3b493384", ch.now_ms);
    with_model(|m| { let _ = m.write(b"pending"); });
    assert!(bridge.output_pending());
    ch.now_ms += 990;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(bridge.output_pending());
    ch.now_ms += 10;
    bridge.poll(&mut ch, &mut dev, "8dad3b493384", true);
    assert!(!bridge.output_pending());
    assert!(bridge.background_work_ready());
}
