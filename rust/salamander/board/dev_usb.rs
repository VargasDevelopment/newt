// SPDX-License-Identifier: MIT
//! Frozen UsbChannel's SDK calls, with no held buffer, task or protocol policy.

// SAFETY: linked native entries forward synchronous SDK Serial observations,
// one bounded write, or the existing nonblocking HAL transmit-tail wake pair.
unsafe extern "C" {
    fn nsc_connected() -> u8;
    fn nsc_available() -> i32;
    fn nsc_read_byte() -> i32;
    fn nsc_available_for_write() -> i32;
    fn nsc_wake_transmitter();
    fn nsc_write(data: *const u8, len: usize) -> usize;
    fn nsc_flush();
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
    // SAFETY: same nonblocking HAL wake that frozen UsbChannel performs before
    // querying room; it neither waits nor discards queued bytes.
    unsafe { nsc_wake_transmitter(); }
    // SAFETY: scalar SDK queue-space observation, retaining no state or pointer.
    let space = unsafe { nsc_available_for_write() };
    if space <= 0 { return 0; }
    let count = bytes.len().min(space as usize);
    // SAFETY: the prefix lives through this one synchronous SDK write. Its
    // reported count is passed back unchanged; it is not a delivery promise.
    let written = unsafe { nsc_write(bytes.as_ptr(), count) };
    if written != 0 {
        // SAFETY: the same post-enqueue transmit-tail wake, with no retry.
        unsafe { nsc_wake_transmitter(); }
    }
    written
}

/// Existing SDK transmit room, with no write or wake side effect.
pub fn available_for_write() -> i32 {
    // SAFETY: scalar SDK observation, retaining no object or buffer.
    unsafe { nsc_available_for_write() }
}
