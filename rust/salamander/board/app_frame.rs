// SPDX-License-Identifier: MIT
//! The firmware's sole BSS logical frame.
//! The SDK's separate primary frame and parser loan remain Board operations.
use super::FRAME_BYTES;

static mut LOGICAL_FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

/// # Safety
/// Use only within the existing exclusive logical-frame loan while the TLS
/// allocator callbacks are installed. This is its fixed owner address, not
/// permission to create a reference or to acquire the frame a second time.
// SAFETY: caller keeps the existing exclusive frame epoch and callback scope.
pub unsafe fn logical_address() -> *mut [u8; FRAME_BYTES] { &raw mut LOGICAL_FRAME }

/// No frame-sized stack temporary, allocation, copy or second backing.
///
/// # Safety
/// This is the firmware's only logical frame. Invoke only on the sole UI task,
/// with no live logical-frame loan,
/// native alias, callback reentry or interrupt access. The callback may lend
/// it sequentially to UI/reader operations but cannot return its reference.
/// These obligations include forgotten loans and last until the callback ends.
pub unsafe fn with_logical<R>(
    operation: impl for<'frame> FnOnce(&'frame mut [u8; FRAME_BYTES]) -> R,
) -> R {
    // SAFETY: caller excludes every other acquisition/access for this epoch.
    // BSS initializes this exact fixed owner before entry, without a stack move.
    operation(unsafe { &mut *(&raw mut LOGICAL_FRAME) })
}
