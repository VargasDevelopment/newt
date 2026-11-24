//! Reading screen dispatch, overlays, chrome and reader owners. MIT; Inter OFL-1.1.
#![no_std]
#![forbid(unsafe_code)]
mod chrome;
mod compose;
mod dispatch;
mod external_owner;
#[cfg(feature = "embedded-sample")]
mod sample_owner;
#[cfg(feature = "embedded-sample")]
pub use sample_owner::SampleOperation;
mod restore_policy;
pub use restore_policy::RestorePolicy;
mod physical_source;
mod source_state;
pub use source_state::ReaderSourceState;
pub use physical_source::{PendingReaderSource, PhysicalSource, PhysicalSourceRef, ReaderSource,
    SourceAuthorizer, SourceOperations, StrictOperations, StrictSource};
mod lifecycle;
mod overlay;
mod owner;
pub use external_owner::*;
mod statistics;
mod working;
pub use chrome::*;
pub use compose::*;
use core::fmt::{self, Write};
pub use dispatch::*;
pub use lifecycle::*;
pub use newt_core::{
    app::Action,
    frame::Frame,
    preferences::{Orientation, TextSize, ViewSettings},
};
use newt_ui_canvas::UiCanvas;
pub use overlay::*;
pub use owner::*;
pub use statistics::*;
pub use working::*;
fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&v| v == 0).unwrap_or(s.len())]
}
struct Buffer<'a> {
    bytes: &'a mut [u8],
    used: usize,
}
impl<'a> Buffer<'a> {
    fn new(bytes: &'a mut [u8]) -> Self {
        if !bytes.is_empty() {
            bytes[0] = 0;
        }
        Self { bytes, used: 0 }
    }
}
impl Write for Buffer<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let n = s.len().min(self.bytes.len().saturating_sub(self.used + 1));
        self.bytes[self.used..self.used + n].copy_from_slice(&s.as_bytes()[..n]);
        self.used += n;
        if self.used < self.bytes.len() {
            self.bytes[self.used] = 0;
        }
        Ok(())
    }
}
fn width(s: &[u8], p: u32, b: bool) -> i32 {
    newt_ui_canvas::text_width(Some(s), p, b)
}
fn text(c: &mut UiCanvas<'_>, x: i32, y: i32, s: &[u8], p: u32, b: bool, w: i32) {
    c.text(x, y, Some(s), p, b, w, true);
}
fn para(c: &mut UiCanvas<'_>, x: i32, y: i32, s: &[u8], p: u32, w: i32, bottom: i32) {
    c.paragraph(x, y, Some(s), p, w, bottom);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
pub fn overlay_geometry(settings: ViewSettings) -> OverlayGeometry {
    let g = newt_core::preferences::geometry(settings);
    let landscape = g.width > g.height;
    OverlayGeometry {
        x: 27,
        y: if landscape { 64 } else { 180 },
        width: if landscape {
            637
        } else {
            i32::from(g.width) - 54
        },
        height: 402,
    }
}

mod retained_paint;
