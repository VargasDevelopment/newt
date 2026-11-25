//! Runtime-borrowed input, ZIP entry, typed decoder state/dictionary/input,
//! staging and output. Link/inspect RV32 separately; no execution claim.
use newt_core::member_check::Checked;
use newt_core::image_probe::{ImageInfo, probe};
use newt_core::zip_container::{InflateScratch, Status};
use newt_core::zip_window::source::BookSource;
struct Borrowed<'a>(&'a [u8]);
impl BookSource for Borrowed<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, dst: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else {
            return -1;
        };
        let tail = self.0.get(at..).unwrap_or(&[]);
        let n = tail.len().min(dst.len());
        dst[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
#[inline(never)]
pub fn borrowed(
    input: &[u8],
    entry: &Checked,
    decode: Option<InflateScratch<'_>>,
    staging: &mut [u8],
    out: &mut ImageInfo,
) -> Status {
    probe(&mut Borrowed(input), entry, decode, staging, out)
}
