// SPDX-License-Identifier: MIT
//! Borrowed RV32 body over the shared native FreeType boundary. Not a board
//! entry, panic owner or default firmware member; only Root's Run compiles it.
#![no_std]
#[path = "../../../firmware/fonts/native.rs"]
pub mod native;
use native::{BookStream, Library, Native, StreamContext};
use newt_core::{ft_font::FtFont, zip_window::source::BookSource};

/// Opens the borrowed source (e.g. a bound FileBookSource) through the pinned
/// FontAlloc heap, exercises metrics, kerning, GSUB and raster reads, then
/// tears down face, stream and library in reverse order before returning.
/// Returns -1 when the library fails, -2 minus InitFailure when the face fails,
/// otherwise a fold of runtime results. Nothing is retained after return.
/// # Safety
/// Same as `Library::font_heap`: initialized SDK heap, ordinary task context.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe fn ft_native_borrowed_runtime(
    source: &mut dyn BookSource,
    mono: &mut [u8],
    gsub: &mut [u8],
    gpos: &mut [u8],
    codepoints: &[u32],
    size: u16,
    copy: &mut [u8],
) -> i64 {
    let Ok(library) = (unsafe { Library::font_heap() }) else {
        return -1;
    };
    let mut stream = BookStream::new(source);
    let length = stream.length();
    let mut context = StreamContext { reader: &mut stream, length };
    let mut font = FtFont::new(Native::stream(&library, &mut context), mono, gsub, gpos);
    if !font.init(None, size, 400, false) {
        return -2 - i64::from(font.last_init_failure as u8);
    }
    let n = u32::from(size) * 64;
    let first = codepoints.first().copied().unwrap_or(0);
    let mut out = 0i64;
    for &cp in codepoints {
        if let Some(m) = font.metrics26_6(cp, n) {
            out = out.wrapping_add(i64::from(m.advance26_6));
        }
        out ^= i64::from(font.kerning26_6(cp, first, n));
        out ^= i64::from(font.ligature_codepoint(cp, first));
        if let Some(b) = font.rasterize26_6(cp, n) {
            let count = copy.len().min(b.pixels.len());
            copy[..count].copy_from_slice(&b.pixels[..count]);
            out ^= (i64::from(b.width) << 32) | i64::from(b.height);
        }
        out = out.wrapping_mul(33).wrapping_add(font.last_glyph_failure as i64);
    }
    out
}
