//! Shared `no_std` UI drawing canvas and Inter font atlas. MIT; Inter OFL-1.1.
#![no_std]
#![forbid(unsafe_code)]
pub mod atlas;
mod data;
pub mod paint;
pub mod text;
pub use atlas::text_width;
pub use paint::{Clip, draw_text};
pub mod canvas;
pub use canvas::UiCanvas;
pub use newt_core::frame::{FRAME_BYTES, Frame, HEIGHT, WIDTH};
pub use text::*;
/// C strings stop at the first NUL; absent pointers are empty for measurement.
pub(crate) fn cstr(value: Option<&[u8]>) -> &[u8] {
    let v = value.unwrap_or(&[]);
    &v[..v.iter().position(|&b| b == 0).unwrap_or(v.len())]
}

/// The two selector bits and the ordinary monochrome frame share one type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomePlane {
    Lsb,
    Msb,
    Monochrome,
}
