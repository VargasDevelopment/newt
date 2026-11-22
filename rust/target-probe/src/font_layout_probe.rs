//! Root must export/link these bodies and inspect nonempty RV32 code.
//! All data, glyphs and output storage are supplied at runtime.
use newt_core::font_layout::{ligature_glyph_id, pair_kern_adjustment};
#[inline(never)]
pub fn pair(table: &[u8], glyphs: &[u32; 2], output: &mut i32) {
    *output = pair_kern_adjustment(table, glyphs[0], glyphs[1]);
}
#[inline(never)]
pub fn ligature(table: &[u8], glyphs: &[u32], output: &mut u32) {
    *output = ligature_glyph_id(table, glyphs);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_font_pair_borrowed(table: &[u8], glyphs: &[u32; 2], output: &mut i32) {
    pair(table, glyphs, output)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_font_ligature_borrowed(table: &[u8], glyphs: &[u32], output: &mut u32) {
    ligature(table, glyphs, output)
}
