//! Borrowed RV32 runtime entry points. Compilation and execution are pending.
use super::{Geometry, ReaderAtlas};
use newt_core::page_renderer::Font;
#[derive(Clone, Copy)]
pub struct Request { pub cp: u32, pub right: u32, pub px: u16, pub style: u8, pub x: i32, pub baseline: i32 }
#[inline(never)]
pub fn reader_atlas_metrics_runtime(font: &mut dyn Font, requests: &[Request]) -> i64 {
    let mut sum = 0i64;
    for r in requests {
        sum = sum.wrapping_mul(33).wrapping_add(i64::from(font.advance(r.cp,r.px,r.style)));
        sum ^= i64::from(font.kerning(r.cp,r.right,r.px,r.style));
        sum = sum.wrapping_add(i64::from(font.ascent(r.px)) + i64::from(font.line_height(r.px)));
        sum ^= i64::from(font.covers(r.cp));
    }
    sum
}
#[inline(never)]
pub fn reader_atlas_draw_runtime(font: &ReaderAtlas, requests: &[Request], geometry: Geometry, frame: &mut [u8]) -> i64 {
    for r in requests {
        if font.draw(frame,geometry,r.x,r.baseline,r.cp,r.px,r.style).is_err() { return -1; }
    }
    frame.iter().fold(0i64, |s,&b| s.wrapping_mul(33).wrapping_add(i64::from(b)))
}
