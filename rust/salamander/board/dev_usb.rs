// SPDX-License-Identifier: MIT
//! Frozen UsbChannel's SDK calls, with no held buffer, task or protocol policy.

/// Arduino HWCDC's default TX ring buffer size (HWCDC::begin).
const HWCDC_TX_RING_BYTES: i32 = 256;

// SAFETY: matching native leaves use SDK Serial normally. The scoped RX
// leaves use the C3 HAL under an exclusive receive token; no pointer is retained.
unsafe extern "C" {
    fn nsc_put_rx_begin() -> u32;
    fn nsc_put_rx_end(previous: u32);
    fn nsc_put_rx_read_byte() -> i32;
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
        // Drop restores RX. Native atomically masks only its interrupt bit.
        let previous = unsafe { nsc_put_rx_begin() };
        PutReceive { previous, _owner: core::marker::PhantomData }
    }
}
const _: () = assert!(core::mem::size_of::<ReceiveOwner>() == 0);

/// One exclusive UI-task receive loan; Drop restores only the prior RX bit.
/// TX remains on HWCDC throughout. No allocation or standing receive state.
pub struct PutReceive<'owner> {
    previous: u32,
    _owner: core::marker::PhantomData<&'owner mut ReceiveOwner>,
}
impl PutReceive<'_> {
    pub fn read_byte(&mut self) -> i32 {
        // SAFETY: this non-Send guard holds RX exclusion until Drop. Native
        // drains the old SDK queue before the same FIFO its ISR normally reads.
        unsafe { nsc_put_rx_read_byte() }
    }
}
impl Drop for PutReceive<'_> {
    fn drop(&mut self) {
        // SAFETY: the guard still holds its exclusive loan. Restore only the
        // saved RX enable bit; leave pending bytes/raw status and TX untouched.
        unsafe { nsc_put_rx_end(self.previous) }
    }
}
