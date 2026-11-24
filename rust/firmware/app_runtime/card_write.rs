// SPDX-License-Identifier: MIT
//! The bridge's card-write lease (newt_core::dev_put) over the mounted card,
//! the bridge's serial loan and the input gate. Its working memory is one heap
//! block that exists only while the lease runs.
#![forbid(unsafe_code)]
use crate::{app_image::{Events, Home}, board_clock::BoardBus, dev_usb::Bridge, serial_console::PutChannel};
use core::ffi::CStr;
use newt_core::dev_put::{self, Card, Entry, Session, Wire, MEMORY_BYTES};
use newt_native_io::OsNative;
use newt_native_storage::{Error, Kind, Native, Open};

const MOUNT: &CStr = c"/newt-sd";

/// Runs one granted lease to its end and hands the outcome to the bridge.
pub fn lease(bridge: &mut Bridge, home: &mut Home<'_>, events: &mut Events<'_, '_>, session: Session) {
    let mut block = newt_sdk_heap::Bytes::try_new(MEMORY_BYTES);
    let Some(memory) = block.as_mut().and_then(|block| <&mut [u8; MEMORY_BYTES]>::try_from(block.as_mut()).ok()) else {
        bridge.put_refused(session.request, b"ERROR no-memory");
        return;
    };
    let mut ended = None;
    let mounted = home.with_storage_io(|home, io| {
        let BoardBus { board, log } = &mut home.bus;
        let clock = board.monotonic_clock();
        let mut card = DevCard(io.port());
        // A failed hash scope leaves `ended` empty: the lease never began.
        let _ = board.with_sha256(|hash| {
            // Acquire only after admission, memory, mount and hash succeeded,
            // before the offer lets the host send any FILE or DATA bytes.
            let mut wire = LeaseWire { channel: log.dev_channel(clock).receive_put(), events, tx_flush_pending: false };
            ended = Some(dev_put::run(session, memory, &mut wire, &mut card, hash));
        });
    });
    let now = home.bus.board.monotonic_clock().millis();
    match (mounted, ended) {
        (_, Some(end)) => bridge.put_ended(end, crate::app_dev::BUILD_ID, now),
        (Ok(()), None) => bridge.put_refused(session.request, b"ERROR hash"),
        (Err(_), None) => bridge.put_refused(session.request, b"ERROR no-card"),
    }
}

struct LeaseWire<'a, 'events, 'task> {
    channel: PutChannel<'a>,
    events: &'a mut Events<'events, 'task>,
    tx_flush_pending: bool,
}
impl Wire for LeaseWire<'_, '_, '_> {
    fn connected(&mut self) -> bool { self.channel.connected() }
    fn read_byte(&mut self) -> i32 {
        let byte = self.channel.read_byte();
        if byte < 0 && self.tx_flush_pending && self.channel.flush_tx_tail() {
            self.tx_flush_pending = false;
        }
        byte
    }
    fn write(&mut self, bytes: &[u8]) -> usize {
        let sent = self.channel.write(bytes);
        if sent != 0 { self.tx_flush_pending = true; }
        sent
    }
    fn now(&mut self) -> u32 { self.channel.now() }
    /// Takes the gate's pending press, as Bridge::drain_pending does.
    fn physical_input(&mut self) -> bool { self.events.task.gate().with_pending(core::mem::take) }
    fn pause(&mut self) { crate::app_power::delay_ms(1); }
}

/// The card as the lease sees it: errno for the raw download leaves, and the
/// nearest errno for a classified native error (-1 when unclassified).
struct DevCard<'ui>(OsNative<'ui>);

fn code(error: Error) -> i32 {
    match error {
        Error::Interrupted => 4,
        Error::Missing => 2,
        Error::Exists => 17,
        Error::Unavailable => 5,
        Error::Cancelled => 125,
        Error::Other => -1,
    }
}

fn text(path: &CStr) -> Result<&str, i32> { path.to_str().map_err(|_| -1) }

impl Card for DevCard<'_> {
    fn entry(&mut self, path: &CStr) -> Result<Entry, i32> {
        match self.0.stat(text(path)?, true) {
            Ok(stat) => Ok(match stat.kind {
                Kind::Regular => Entry::File,
                Kind::Directory => Entry::Folder,
                Kind::Other => Entry::Other,
            }),
            Err(Error::Missing) => Ok(Entry::Missing),
            Err(error) => Err(code(error)),
        }
    }
    fn make_folder(&mut self, path: &CStr) -> Result<(), i32> { self.0.mkdir_mode_raw(path, 0o755) }
    fn create(&mut self, path: &CStr) -> Result<i32, i32> { self.0.open_truncate_raw(path, 0o644) }
    fn open_read(&mut self, path: &CStr) -> Result<i32, i32> {
        self.0.open(text(path)?, Open::SourceReadOnly).map_err(code)
    }
    fn write(&mut self, fd: i32, bytes: &[u8]) -> Result<usize, i32> { self.0.write_raw(fd, bytes) }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize, i32> { Native::read(&mut self.0, fd, out).map_err(code) }
    fn sync(&mut self, fd: i32) -> Result<(), i32> { self.0.sync_raw(fd) }
    fn close(&mut self, fd: i32) -> Result<(), i32> { self.0.close_raw(fd) }
    fn remove(&mut self, path: &CStr) -> Result<(), i32> { self.0.unlink_raw(path) }
    fn rename(&mut self, from: &CStr, to: &CStr) -> Result<(), i32> { self.0.rename_cstr(from, to).map_err(code) }
    fn free_bytes(&mut self) -> Result<u64, i32> { self.0.free_bytes_raw(MOUNT) }
}
