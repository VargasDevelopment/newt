//! Host-only observation seam. No alternate helper algorithms or allocation.
use crate::span::{self, Span};
use newt_ui_canvas::atlas::Face;
pub struct SpanObservation {
    pub bytes: usize,
    pub width64: i32,
    pub fits: bool,
    pub copied: [u8; 128],
    pub fitted: [u8; 128],
}
pub fn observe_span(
    text: &[u8],
    suffix: &[u8],
    joined: bool,
    face: Face,
    width: i32,
    mut fit_input: [u8; 128],
    mut codepoint: impl FnMut(u32),
) -> SpanObservation {
    let value = if joined {
        Span::joined(text, suffix)
    } else {
        Span::plain(text)
    };
    value.spell(&mut codepoint);
    span::fit(&mut fit_input, face, width);
    SpanObservation {
        bytes: value.bytes(),
        width64: value.width(face),
        fits: value.fits(face, width),
        copied: value.copy(),
        fitted: fit_input,
    }
}

pub use crate::canvas::observe_repeated_text;
