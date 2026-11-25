//! Link/inspect on RV32 separately. Rust ABI exports take runtime borrows.
//! No FFI layout or hardware execution claim is made.
use newt_core::member_check::Checked;
use newt_core::zip_container::{InflateScratch, Status};
use newt_core::css::{CssDecl, CssRule};
use crate::css_borrowed as probes;
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_css_text_borrowed(
    css: &[u8],
    slots: &mut [CssRule],
    elem: Option<&[u8]>,
    classes: Option<&[u8]>,
    inline: Option<&[u8]>,
) -> (bool, u32, usize, CssDecl) {
    probes::text(css, slots, elem, classes, inline)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_css_sheet_borrowed(
    input: &[u8],
    entry: &Checked,
    slots: &mut [CssRule],
    decode: Option<InflateScratch<'_>>,
    text: &mut [u8],
    elem: Option<&[u8]>,
    classes: Option<&[u8]>,
) -> (Status, u32, usize, u16, CssDecl) {
    probes::sheet(input, entry, slots, decode, text, elem, classes)
}
