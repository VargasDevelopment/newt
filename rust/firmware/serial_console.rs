//! MIT. Firmware text sink over the SDK's USB Serial: `Serial.begin(115200)`,
//! then, in DEV builds only, `Serial.setTxTimeoutMs(5)`. The native side is
//! serial_console/native. Each `write_str` is one SDK write, which holds the
//! SDK's transmit lock for the whole call, as one pinned `printf` does; a
//! caller stages what it writes together (write_clock_log stages each of the
//! pinned two halves). Unsafe is confined to the bridge calls.
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

/// The begun Serial. Zero-sized; only `begin` makes one, so no write through
/// it can precede the SDK's begin. !Send and !Sync: it stays on the setup task
/// that began it and moves into that task's BoardBus/PanelBus `log`.
pub struct SerialConsole {
    _task: PhantomData<*mut ()>,
    #[cfg(feature = "dev-usb")]
    receive: newt_c3_board::dev_usb::ReceiveOwner,
}

impl SerialConsole {
    pub fn available(&self) -> i32 {
        newt_c3_board::dev_usb::available()
    }

    // This UI-only ZST owns no Rust buffer; SDK writes also share its input probe.
    pub fn write_bytes(&self, bytes: &[u8]) -> usize {
        // SAFETY: bytes stays live for this synchronous existing SDK write.
        unsafe { nsc_write(bytes.as_ptr(), bytes.len()) }
    }

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
            }
        }
    }

    /// The same exclusive serial owner, borrowed only for this bridge poll.
    /// Logging cannot share the port while the channel's loan is active.
    #[cfg(feature = "dev-usb")]
    pub fn dev_channel(&mut self, clock: newt_c3_board::MonotonicClock) -> DevChannel<'_> {
        DevChannel { _serial: self, clock }
    }
}

#[cfg(feature = "dev-usb")]
pub struct DevChannel<'a> {
    _serial: &'a mut SerialConsole,
    clock: newt_c3_board::MonotonicClock,
}

#[cfg(feature = "dev-usb")]
impl<'a> DevChannel<'a> {
    /// Consume the sole serial loan for a granted PUT. Other bridge commands
    /// keep the normal SDK receive path and cannot nest another RX loan.
    pub fn receive_put(self) -> PutChannel<'a> {
        PutChannel { rx: self._serial.receive.receive(), clock: self.clock }
    }
    pub fn connected(&mut self) -> bool {
        newt_c3_board::dev_usb::connected()
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
    clock: newt_c3_board::MonotonicClock,
}
#[cfg(feature = "dev-usb")]
impl PutChannel<'_> {
    pub fn connected(&mut self) -> bool { newt_c3_board::dev_usb::connected() }
    pub fn read_byte(&mut self) -> i32 { self.rx.read_byte() }
    pub fn flush_tx_tail(&mut self) -> bool { newt_c3_board::dev_usb::flush_tx_tail() }
    pub fn write(&mut self, bytes: &[u8]) -> usize { newt_c3_board::dev_usb::write(bytes) }
    pub fn now(&mut self) -> u32 { self.clock.millis() }
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
