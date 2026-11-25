// SPDX-License-Identifier: MIT
//! Frozen UsbChannel's SDK calls, with no held buffer, task or protocol policy.

/// Arduino HWCDC's default TX ring buffer size (HWCDC::begin).
const HWCDC_TX_RING_BYTES: i32 = 256;

// SAFETY: matching native leaves use SDK Serial normally. The scoped RX
// leaves use the C3 HAL under an exclusive receive token; no pointer is retained.
unsafe extern "C" {
    fn nsc_put_rx_begin() -> u32;
    fn nsc_put_rx_end(previous: u32);
    fn nsc_put_rx_read(out: *mut u8, max_len: usize, queue_empty: *mut u8) -> usize;
    fn nsc_connected() -> u8;
    fn nsc_available() -> i32;
    fn nsc_read_byte() -> i32;
    fn nsc_available_for_write() -> i32;
    fn nsc_wake_transmitter() -> u8;
    fn nsc_write(data: *const u8, len: usize) -> usize;
    fn nsc_flush();
}

fn wake_transmitter() -> bool {
    // SAFETY: nonblocking HAL transmit-tail wake; it neither waits nor
    // discards queued bytes, and reports whether the HW TX FIFO had room.
    unsafe { nsc_wake_transmitter() != 0 }
}

pub fn connected() -> bool {
    // SAFETY: scalar SDK observation; false for the disconnected/unbegun port.
    unsafe { nsc_connected() != 0 }
}

pub fn available() -> i32 {
    // SAFETY: scalar SDK receive count; no byte is consumed or retained.
    unsafe { nsc_available() }
}

pub fn flush() {
    // SAFETY: the begun Serial owner calls this synchronously at shutdown.
    unsafe { nsc_flush() }
}

pub fn read_byte() -> i32 {
    // SAFETY: at most one synchronous SDK read after its availability query.
    // Native retains no pointer, and the SDK owns the initialized serial queue.
    unsafe { if nsc_available() > 0 { nsc_read_byte() } else { -1 } }
}

pub fn write(bytes: &[u8]) -> usize {
    if !connected() { return 0; }
    let _ = wake_transmitter();
    let space = available_for_write();
    if space <= 0 { return 0; }
    let count = bytes.len().min(space as usize);
    // SAFETY: the prefix lives through this one synchronous SDK write. Its
    // reported count is passed back unchanged; it is not a delivery promise.
    let written = unsafe { nsc_write(bytes.as_ptr(), count) };
    if written != 0 {
        let _ = wake_transmitter();
    }
    written
}

/// Flushes a trailing 64-byte USB full-packet boundary once HWCDC's TX ring
/// buffer is empty and the hardware EP1 IN FIFO has room to send a zero-length
/// packet. Returns true once accepted or disconnected.
pub fn flush_tx_tail() -> bool {
    if !connected() { return true; }
    if available_for_write() < HWCDC_TX_RING_BYTES {
        let _ = wake_transmitter();
        return false;
    }
    wake_transmitter()
}

/// Existing SDK transmit room, with no write or wake side effect.
pub fn available_for_write() -> i32 {
    // SAFETY: scalar SDK observation, retaining no object or buffer.
    unsafe { nsc_available_for_write() }
}

/// The sole begun Serial's receive authority. No memory or hardware state is
/// held; borrowing it prevents concurrent or nested receive loans.
pub struct ReceiveOwner(core::marker::PhantomData<*mut ()>);
impl ReceiveOwner {
    /// # Safety
    /// Mint once from the sole begun Serial owner on its UI task.
    pub unsafe fn acquired() -> Self { Self(core::marker::PhantomData) }
    pub fn receive(&mut self) -> PutReceive<'_> {
        // SAFETY: the mutable authority loan excludes another receiver until
        // Drop restores RX. Native atomically masks only its interrupt bit
        // before PutReceive starts with queue_empty = 0.
        let previous = unsafe { nsc_put_rx_begin() };
        PutReceive { previous, queue_empty: 0, _owner: core::marker::PhantomData }
    }
}
const _: () = assert!(core::mem::size_of::<ReceiveOwner>() == 0);

/// One exclusive UI-task receive loan; Drop restores only the prior RX bit.
/// TX remains on HWCDC throughout. No allocation or standing receive state.
///
/// Ordering proof for `queue_empty`: `ReceiveOwner::receive` masks
/// `USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT` before constructing `PutReceive`
/// with `queue_empty = 0`. Only HWCDC's masked RX ISR branch pushes into the
/// SDK `rx_queue`, and this `!Send`/`!Sync` guard keeps RX masked until `Drop`
/// resets `queue_empty` and destroys it (on completion, takeover, timeout, or
/// error). Once the first drain observes `rx_queue` empty under that mask and
/// sets `queue_empty = 1`, no byte can enter `rx_queue` for the rest of this
/// guard's life; every new lease builds a fresh guard with `queue_empty = 0`.
pub struct PutReceive<'owner> {
    previous: u32,
    queue_empty: u8,
    _owner: core::marker::PhantomData<&'owner mut ReceiveOwner>,
}
impl PutReceive<'_> {
    pub fn read(&mut self, out: &mut [u8], mut micros: impl FnMut() -> u32) -> usize {
        if out.is_empty() { return 0; }
        // Fast path: drain queued/FIFO bytes without touching the timer.
        // USB packets within a burst arrive between ticks; briefly poll an
        // empty FIFO before Core sleeps so control returns to Core's deadline.
        const QUIET_US: u32 = 250;
        let mut started: Option<u32> = None;
        loop {
            // SAFETY: this non-Send guard holds RX exclusion until Drop. Native
            // drains the SDK queue before the hardware FIFO, writes at most
            // `out.len()` bytes, and retains no pointer.
            let got = unsafe { nsc_put_rx_read(out.as_mut_ptr(), out.len(), &mut self.queue_empty) };
            if got > 0 { return got; }
            let now = micros();
            let start = *started.get_or_insert(now);
            if now.wrapping_sub(start) >= QUIET_US { return 0; }
        }
    }
    pub fn read_byte(&mut self, micros: impl FnMut() -> u32) -> i32 {
        let mut byte = [0u8; 1];
        if self.read(&mut byte, micros) == 1 { i32::from(byte[0]) } else { -1 }
    }
}
impl Drop for PutReceive<'_> {
    fn drop(&mut self) {
        // Clear before lifting the RX mask so no dropped state can carry a
        // stale empty flag; the next receive() also starts at 0.
        self.queue_empty = 0;
        // SAFETY: the guard still holds its exclusive loan. Restore only the
        // saved RX enable bit; leave pending bytes/raw status and TX untouched.
        unsafe { nsc_put_rx_end(self.previous) }
    }
}
