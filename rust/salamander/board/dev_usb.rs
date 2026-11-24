// SPDX-License-Identifier: MIT
//! Frozen UsbChannel's SDK calls, with no held buffer, task or protocol policy.

/// Arduino HWCDC's default TX ring buffer size (HWCDC::begin).
const HWCDC_TX_RING_BYTES: i32 = 256;

// SAFETY: linked native entries forward synchronous SDK Serial observations,
// one bounded write, or the existing nonblocking HAL transmit-tail wake pair.
unsafe extern "C" {
    fn nsc_rx_capacity() -> usize;
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

/// DEV receive queue allocated before Serial's interrupt starts; zero if allocation failed.
pub fn rx_capacity() -> usize {
    // SAFETY: scalar result of the begun SDK port's queue allocation.
    unsafe { nsc_rx_capacity() }
}
