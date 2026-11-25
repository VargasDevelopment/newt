//! Runtime-borrowed no_std probe bodies. Parent must export/link and inspect
//! RV32 artifacts; these functions alone are not evidence of target execution.
use newt_core::member_check::Checked;
use newt_core::css::*;
use newt_core::zip_container::{InflateScratch, Status};
use newt_core::zip_window::source::BookSource;
struct Borrowed<'a>(&'a [u8]);
impl BookSource for Borrowed<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else {
            return -1;
        };
        let tail = self.0.get(at..).unwrap_or(&[]);
        let n = tail.len().min(out.len());
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
#[inline(never)]
pub fn text(
    css: &[u8],
    slots: &mut [CssRule],
    elem: Option<&[u8]>,
    classes: Option<&[u8]>,
    inline: Option<&[u8]>,
) -> (bool, u32, usize, CssDecl) {
    let mut b = CssStylesheetBuilder::default();
    let ok = b.begin(slots);
    b.add_text(css);
    let sheet = b.finish();
    let d = parse_inline_style(inline);
    (
        ok,
        sheet.content_hash,
        sheet.rules.len(),
        cascade_for(&sheet, elem, classes, Some(&d)),
    )
}
#[inline(never)]
pub fn sheet(
    input: &[u8],
    entry: &Checked,
    slots: &mut [CssRule],
    decode: Option<InflateScratch<'_>>,
    text: &mut [u8],
    elem: Option<&[u8]>,
    classes: Option<&[u8]>,
) -> (Status, u32, usize, u16, CssDecl) {
    let mut b = CssStylesheetBuilder::default();
    b.begin(slots);
    let status = b.add_sheet(&mut Borrowed(input), entry, decode, text);
    let sheet = b.finish();
    (
        status,
        sheet.content_hash,
        sheet.rules.len(),
        b.skipped_sheets(),
        cascade_for(&sheet, elem, classes, None),
    )
}
