// SPDX-License-Identifier: MIT
//! What the device is, as plain data Newt may read. No natives: shared crates
//! depend on this, never on Salamander's native crates. The X3 is the only
//! device; real interfaces wait for a second one (#292).
#![no_std]
#![forbid(unsafe_code)]

/// An e-ink panel as its controller scans it: `native_height` rows of
/// `native_width` one-bit pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Display {
    pub native_width: usize,
    pub native_height: usize,
}

/// The Xteink X3 panel. Newt draws it portrait, a quarter turn from native.
pub const DISPLAY: Display = Display { native_width: 792, native_height: 528 };
