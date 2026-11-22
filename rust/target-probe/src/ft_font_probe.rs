//! Borrowed runtime probe body; instantiate the wrapper with the actual target
//! backend when available. This file supplies no fake target FreeType backend.
use newt_core::page_renderer::RasterFont;
#[derive(Clone, Copy)]
pub struct Request {
    pub codepoint: u32,
    pub size: u16,
    pub style: u8,
}
/// Non-generic function: target codegen must retain runtime trait calls and bitmap reads.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn ft_borrowed_raster_runtime(
    font: &mut dyn RasterFont,
    requests: &[Request],
    copy: &mut [u8],
) -> i64 {
    let mut result = 0i64;
    for r in requests {
        result = result.wrapping_add(i64::from(font.advance(r.codepoint, r.size, r.style)));
        result ^= i64::from(font.line_height(r.size));
        if let Some(b) = font.rasterize(r.codepoint, r.size) {
            let n = copy.len().min(b.pixels.len());
            copy[..n].copy_from_slice(&b.pixels[..n]);
            for &p in &copy[..n] {
                result = result.wrapping_mul(33).wrapping_add(i64::from(p));
            }
            result ^= (i64::from(b.width) << 32) | i64::from(b.height);
        }
    }
    result
}
/// Full wrapper probe uses caller-owned backend, resident bytes, three scratch
/// slices and runtime request values. No static sample or allocation is involved.
pub fn borrowed_wrapper_probe<'a, B: newt_core::ft_font::Backend<'a>>(
    backend: B,
    bytes: &'a [u8],
    mono: &mut [u8],
    sub: &mut [u8],
    pos: &mut [u8],
    requests: &[Request],
) -> i64 {
    let mut font = newt_core::ft_font::FtFont::new(backend, mono, sub, pos);
    let Some(first) = requests.first() else {
        return 0;
    };
    if !font.init(Some(bytes), first.size, 400, false) {
        return -1;
    }
    let mut out = 0i64;
    for r in requests {
        let n = u32::from(r.size) * 64;
        if let Some(m) = font.metrics26_6(r.codepoint, n) {
            out = out.wrapping_add(i64::from(m.advance26_6));
        }
        out ^= i64::from(font.kerning26_6(r.codepoint, first.codepoint, n));
        out ^= i64::from(font.ligature_glyph_id(&[r.codepoint, first.codepoint]));
    }
    let mut copy = [0u8; 16];
    out.wrapping_add(ft_borrowed_raster_runtime(&mut font, requests, &mut copy))
}

/// Pure policy only: no native target backend or firmware claim.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn ft_policy_runtime(bytes: &[u8], tag: u32, options: newt_core::ft_font::RenderOptions) -> i32 {
    let mut result = newt_core::ft_font::load_flags(options);
    if let Some(table) = newt_core::ft_font::sfnt_table(bytes, tag) {
        for &byte in table {
            result = result.wrapping_mul(33).wrapping_add(i32::from(byte));
        }
    }
    result
}
