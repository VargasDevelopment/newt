// SPDX-License-Identifier: MIT
//! Packed X3 frame transforms and regional selector patches. MIT. See LICENSE.
//!
//! The caller owns both complete frames. Safe borrows exclude aliasing and
//! short buffers; no display refresh or generation is published here.

use crate::preferences::Orientation;
use salamander_device::DISPLAY;

/// Newt's portrait frame: the device's panel, a quarter turn from native.
pub const WIDTH: usize = DISPLAY.native_height;
pub const HEIGHT: usize = DISPLAY.native_width;
pub const NATIVE_WIDTH: usize = DISPLAY.native_width;
pub const NATIVE_HEIGHT: usize = DISPLAY.native_height;
pub const FRAME_BYTES: usize = WIDTH * HEIGHT / 8;
pub type Frame = [u8; FRAME_BYTES];

/// Whether `width`×`height` is the frame, held portrait or landscape.
pub const fn is_frame_size(width: usize, height: usize) -> bool {
    matches!((width, height), (WIDTH, HEIGHT) | (HEIGHT, WIDTH))
}

/// Native coordinates; each region must cover complete packed bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeRegion {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// Copy, render synchronously, then publish only a completed logical frame.
/// A rejected render may alter staging, but leaves published byte-identical.
pub fn publish_staged_frame(
    published: &mut Frame,
    staging: &mut Frame,
    render: impl FnOnce(&mut Frame) -> bool,
) -> bool {
    staging.copy_from_slice(published);
    if !render(staging) {
        return false;
    }
    published.copy_from_slice(staging);
    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum ActionFrameResult {
    Published,
    /// Scratch use may have overwritten the logical frame. Repaint the whole
    /// frame before capture, conversion, refresh, or any use as published pixels.
    RepaintRequired,
}

/// Core memory contract of publishLendingFrame, with callback-scoped scratch.
/// The callback receives (staging, action_storage), which are distinct frames.
/// No reference may escape the callback.
///
/// ```compile_fail
/// use newt_core::frame::*;
/// let mut published = [0; FRAME_BYTES];
/// let mut staging = [0; FRAME_BYTES];
/// let mut escaped = None;
/// publish_with_action_storage(&mut published, &mut staging, |_, action| {
///     escaped = Some(action); false
/// });
/// drop(escaped);
/// ```
///
/// Failure conservatively needs repaint,
/// even when this particular callback left its action storage untouched.
///
/// This does not implement ImportedRevision's owner lending/withdrawal API:
/// its future adapter must pass scratch into reads for this scope, invalidate
/// retained block tags on entry/exit, and restore owned scratch on every exit.
pub fn publish_with_action_storage(
    published: &mut Frame,
    staging: &mut Frame,
    render: impl FnOnce(&mut Frame, &mut Frame) -> bool,
) -> ActionFrameResult {
    staging.copy_from_slice(published);
    if !render(staging, published) {
        return ActionFrameResult::RepaintRequired;
    }
    published.copy_from_slice(staging);
    ActionFrameResult::Published
}

/// Restore only the borrowed prefix from unchanged logical PBM pixels.
/// Oversized requests fail before writing; every remaining byte is preserved.
pub fn restore_native_frame_prefix(
    logical: &Frame,
    native: &mut Frame,
    bytes: usize,
    orientation: Orientation,
) -> bool {
    if bytes > FRAME_BYTES {
        return false;
    }
    if landscape(logical, &mut native[..bytes], orientation, |byte| !byte) {
        return true;
    }
    const LOGICAL_ROW_BYTES: usize = WIDTH / 8;
    const NATIVE_ROW_BYTES: usize = NATIVE_WIDTH / 8;
    const { assert!(WIDTH % 8 == 0 && NATIVE_WIDTH % 8 == 0) };
    // One native row selects one logical column. Each output byte gathers
    // the same bit from eight logical rows, so coordinate work is shared.
    for (row, output_row) in native[..bytes].chunks_mut(NATIVE_ROW_BYTES).enumerate() {
        let source_column = WIDTH - 1 - row;
        let column_byte = source_column / 8;
        let source_mask = 0x80u8 >> (source_column % 8);
        for (eight_rows, output) in logical.chunks_exact(8 * LOGICAL_ROW_BYTES).zip(output_row) {
            let mut value = 0xff;
            for (bit, logical_row) in eight_rows.chunks_exact(LOGICAL_ROW_BYTES).enumerate() {
                if logical_row[column_byte] & source_mask != 0 {
                    value &= !(0x80 >> bit);
                }
            }
            *output = value;
        }
    }
    true
}

/// Landscape frames are whole native rows: counterclockwise is the native
/// raster and clockwise is that raster turned half a turn, which reverses
/// both the byte order and the bits in each byte. Fills a native prefix of
/// any length. False for portrait, which the caller turns a quarter.
fn landscape(logical: &Frame, native: &mut [u8], orientation: Orientation, ink: impl Fn(u8) -> u8) -> bool {
    match orientation {
        Orientation::Portrait => return false,
        Orientation::LandscapeCcw => {
            for (output, &input) in native.iter_mut().zip(logical) {
                *output = ink(input);
            }
        }
        Orientation::LandscapeCw => {
            for (output, &input) in native.iter_mut().zip(logical.iter().rev()) {
                *output = ink(input.reverse_bits());
            }
        }
    }
    true
}

/// Rotate gray selector bits without changing their polarity.
pub fn to_native_selectors(logical: &Frame, native: &mut Frame, orientation: Orientation) {
    if landscape(logical, native, orientation, |byte| byte) {
        return;
    }
    native.fill(0);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let source = y * WIDTH + x;
            if logical[source / 8] & (0x80 >> (source % 8)) == 0 {
                continue;
            }
            let target = (WIDTH - 1 - x) * NATIVE_WIDTH + y;
            native[target / 8] |= 0x80 >> (target % 8);
        }
    }
}

/// Patch a bounded native selector plan, preserving every byte outside it.
/// Validate the entire plan before writing, including later invalid regions.
/// Regions can overlap. An empty plan is rejected.
pub fn to_native_selector_regions<const COUNT: usize>(
    portrait: &Frame,
    native: &mut Frame,
    regions: &[NativeRegion; COUNT],
) -> bool {
    if COUNT == 0 {
        return false;
    }
    for region in regions {
        let (x, y, width, height) = (
            usize::from(region.x),
            usize::from(region.y),
            usize::from(region.width),
            usize::from(region.height),
        );
        if width == 0
            || height == 0
            || x % 8 != 0
            || width % 8 != 0
            || x >= NATIVE_WIDTH
            || y >= NATIVE_HEIGHT
            || width > NATIVE_WIDTH - x
            || height > NATIVE_HEIGHT - y
        {
            return false;
        }
    }
    for region in regions {
        let (x, y, width, height) = (
            usize::from(region.x),
            usize::from(region.y),
            usize::from(region.width),
            usize::from(region.height),
        );
        for row in y..y + height {
            for column in (x..x + width).step_by(8) {
                let mut byte = 0;
                let source = column * WIDTH + WIDTH - 1 - row;
                for bit in 0..8 {
                    let at = source + bit * WIDTH;
                    if portrait[at / 8] & (0x80 >> (at % 8)) != 0 {
                        byte |= 0x80 >> bit;
                    }
                }
                native[row * NATIVE_WIDTH / 8 + column / 8] = byte;
            }
        }
    }
    true
}

/// MSB-first logical PBM uses 1=black; native FreeInk uses 0=black.
///
/// ```compile_fail
/// use newt_core::{frame::*, preferences::Orientation};
/// let mut pixels = [0; FRAME_BYTES];
/// to_native_frame(&pixels, &mut pixels, Orientation::Portrait);
/// ```
pub fn to_native_frame(logical: &Frame, native: &mut Frame, orientation: Orientation) {
    if landscape(logical, native, orientation, |byte| !byte) {
        return;
    }
    native.fill(0xff);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let source = y * WIDTH + x;
            if logical[source / 8] & (0x80 >> (source % 8)) == 0 {
                continue;
            }
            let target = (WIDTH - 1 - x) * NATIVE_WIDTH + y;
            native[target / 8] &= !(0x80 >> (target % 8));
        }
    }
}

/// Black an in-bounds portrait rectangle directly in the native BW frame.
/// Rejects out-of-bounds coordinates atomically without writing any pixels.
pub fn fill_native_rect(native: &mut Frame, left: i32, top: i32, width: i32, height: i32) -> bool {
    if left < 0 || top < 0 || width < 0 || height < 0 {
        return false;
    }
    let (left, top, width, height) = (left as usize, top as usize, width as usize, height as usize);
    if left > WIDTH || top > HEIGHT || width > WIDTH - left || height > HEIGHT - top {
        return false;
    }
    for y in top..top + height {
        for x in left..left + width {
            let target = (WIDTH - 1 - x) * NATIVE_WIDTH + y;
            native[target / 8] &= !(0x80 >> (target % 8));
        }
    }
    true
}
