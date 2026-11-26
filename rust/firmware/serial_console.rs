//! MIT. Firmware text sink over the SDK's USB Serial: `Serial.begin(115200)`,
//! then, in DEV builds only, `Serial.setTxTimeoutMs(5)`. The native side is
//! serial_console/native. Each `write_str` is one SDK write, which holds the
//! SDK's transmit lock for the whole call, as one pinned `printf` does; a
//! caller stages what it writes together (write_clock_log stages each of the
//! pinned two halves). In DEV builds a row longer than the transmit ring is
//! written in ring-sized parts. Unsafe is confined to the bridge calls.
use core::{fmt, marker::PhantomData};

unsafe extern "C" {
    fn nsc_begin();
    #[cfg(feature = "dev-usb")]
    fn nsc_set_tx_timeout_ms(ms: u32);
    fn nsc_write(data: *const u8, len: usize) -> usize;
}

/// main.cpp's DEV transmit bound. A blocked write gives up after a few ms
/// instead of the SDK's 100 ms default when no host drains USB.
#[cfg(feature = "dev-usb")]
const DEV_TX_TIMEOUT_MS: u32 = 5;

/// The begun Serial. Only `begin` makes one, so no write through it can
/// precede the SDK's begin. !Send and !Sync: it stays on the setup task that
/// began it and moves into that task's BoardBus/PanelBus `log`. It moves by
/// value through startup, so each byte here costs stack many times over: its
/// only DEV state is the 2 B skipped-row count. The link watch lives with the
/// owner that polls the bridge (#170).
pub struct SerialConsole {
    _task: PhantomData<*mut ()>,
    #[cfg(feature = "dev-usb")]
    receive: newt_c3_board::dev_usb::ReceiveOwner,
    #[cfg(feature = "dev-usb")]
    skipped: core::cell::Cell<u16>,
}

impl SerialConsole {
    pub fn available(&self) -> i32 {
        newt_c3_board::dev_usb::available()
    }

    // This UI-only owner holds no Rust buffer; SDK writes also share its input probe.
    #[cfg(not(feature = "dev-usb"))]
    pub fn write_bytes(&self, bytes: &[u8]) -> usize { sdk_write(bytes) }

    /// DEV: HWCDC is never offered more than its ring has room for. A write
    /// that waits inside HWCDC for 5 ms latches its `connected` flag false,
    /// which ended DEV sessions while the cable stayed in (#170). Wait up to
    /// that same 5 ms for room here instead; if none comes, skip the rest of
    /// the row and count it. While the flag is already false, write as before.
    #[cfg(feature = "dev-usb")]
    pub fn write_bytes(&self, bytes: &[u8]) -> usize {
        use newt_c3_board::dev_usb;
        let mut sent = 0;
        while sent < bytes.len() {
            let part = &bytes[sent..sent + (bytes.len() - sent).min(dev_usb::TX_RING_BYTES)];
            if dev_usb::connected() && !dev_usb::wait_for_room(part.len(), DEV_TX_TIMEOUT_MS * 1_000) {
                self.skipped.set(self.skipped.get().wrapping_add(1));
                break;
            }
            let put = sdk_write(part);
            sent += put;
            if put != part.len() { break; }
        }
        sent
    }

    /// Rows skipped for want of ring room, for the `USB link` row. It wraps.
    #[cfg(feature = "dev-usb")]
    pub fn skipped(&self) -> u16 { self.skipped.get() }

    pub fn flush(&mut self) {
        newt_c3_board::dev_usb::flush();
    }

    /// # Safety
    /// Call once, from firmware setup, before anything else uses the SDK
    /// Serial, and link serial_console.cpp built with the firmware's USB flags.
    /// The DEV bound is the SDK's one global value, for every Serial user.
    pub unsafe fn begin() -> Self {
        // SAFETY: the caller's contract. Each call returns synchronously and
        // retains no Rust pointer. The receive authority is zero-sized.
        unsafe {
            nsc_begin();
            #[cfg(feature = "dev-usb")]
            nsc_set_tx_timeout_ms(DEV_TX_TIMEOUT_MS);
            Self { _task: PhantomData,
                #[cfg(feature = "dev-usb")]
                receive: newt_c3_board::dev_usb::ReceiveOwner::acquired(),
                #[cfg(feature = "dev-usb")]
                skipped: core::cell::Cell::new(0),
            }
        }
    }

    /// The same exclusive serial owner, borrowed only for this bridge poll.
    /// Logging cannot share the port while the channel's loan is active.
    /// `link` is the caller's watch, which decides when the host is gone.
    #[cfg(feature = "dev-usb")]
    pub fn dev_channel<'a>(&'a mut self, clock: newt_c3_board::MonotonicClock,
        link: &'a mut newt_core::dev_link::LinkWatch) -> DevChannel<'a> {
        DevChannel { _serial: self, link, clock }
    }
}

/// One synchronous SDK write; every console write goes through it.
fn sdk_write(bytes: &[u8]) -> usize {
    // SAFETY: bytes stays live for this synchronous existing SDK write.
    unsafe { nsc_write(bytes.as_ptr(), bytes.len()) }
}

#[cfg(feature = "dev-usb")]
pub struct DevChannel<'a> {
    _serial: &'a mut SerialConsole,
    link: &'a mut newt_core::dev_link::LinkWatch,
    clock: newt_c3_board::MonotonicClock,
}

#[cfg(feature = "dev-usb")]
impl<'a> DevChannel<'a> {
    /// Consume the sole serial loan for a granted PUT. Other bridge commands
    /// keep the normal SDK receive path and cannot nest another RX loan.
    pub fn receive_put(self) -> PutChannel<'a> {
        PutChannel { rx: self._serial.receive.receive(), link: self.link, clock: self.clock }
    }
    /// Whether the host link is up: see `newt_core::dev_link` (#170).
    pub fn connected(&mut self) -> bool {
        link_up(self.link, self.clock)
    }
    pub fn read_byte(&mut self) -> i32 {
        newt_c3_board::dev_usb::read_byte()
    }
    pub fn write(&mut self, bytes: &[u8]) -> usize {
        newt_c3_board::dev_usb::write(bytes)
    }
    pub fn flush_tx_tail(&mut self) -> bool {
        newt_c3_board::dev_usb::flush_tx_tail()
    }
    pub fn now(&mut self) -> u32 { self.clock.millis() }
}

/// The active PUT's receive token cannot outlive the exclusive serial loan.
#[cfg(feature = "dev-usb")]
pub struct PutChannel<'a> {
    rx: newt_c3_board::dev_usb::PutReceive<'a>,
    link: &'a mut newt_core::dev_link::LinkWatch,
    clock: newt_c3_board::MonotonicClock,
}
#[cfg(feature = "dev-usb")]
impl PutChannel<'_> {
    pub fn connected(&mut self) -> bool { link_up(self.link, self.clock) }
    pub fn read(&mut self, out: &mut [u8]) -> usize {
        let clock = self.clock;
        self.rx.read(out, || clock.micros())
    }
    pub fn flush_tx_tail(&mut self) -> bool { newt_c3_board::dev_usb::flush_tx_tail() }
    pub fn write(&mut self, bytes: &[u8]) -> usize { newt_c3_board::dev_usb::write(bytes) }
    pub fn now(&mut self) -> u32 { self.clock.millis() }
}

/// One link sample. HWCDC's flag is read first: reading it while false also
/// re-arms the interrupt that sets it true again.
#[cfg(feature = "dev-usb")]
fn link_up(link: &mut newt_core::dev_link::LinkWatch, clock: newt_c3_board::MonotonicClock) -> bool {
    let latched = newt_c3_board::dev_usb::connected();
    link.sample(newt_c3_board::dev_usb::link_present(), latched, clock.millis())
}

impl fmt::Write for SerialConsole {
    /// One SDK write for all of `s`, never retried. Err when the SDK reports
    /// fewer bytes, as after a lock or send timeout; what it took may still be
    /// sent. An empty string makes no SDK call and is Ok.
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let accepted = self.write_bytes(s.as_bytes());
        if accepted == s.len() {
            Ok(())
        } else {
            Err(fmt::Error)
        }
    }
}

/// A complete serial row: bounded formatting fails before any USB write.
pub(crate) struct Row<'a> { pub(crate) bytes: &'a mut [u8], pub(crate) length: usize }
impl<'a> Row<'a> {
    pub(crate) fn new(bytes: &'a mut [u8]) -> Self { Self { bytes, length: 0 } }
}
impl fmt::Write for Row<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.length.checked_add(text.len()).ok_or(fmt::Error)?;
        let out = self.bytes.get_mut(self.length..end).ok_or(fmt::Error)?;
        out.copy_from_slice(text.as_bytes()); self.length = end; Ok(())
    }
}
