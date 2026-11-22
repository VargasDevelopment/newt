//! Rust ABI only; no C ABI layout or target execution claim.
use newt_core::image_probe::ImageInfo;
use newt_core::zip_container::{Entry, InflateScratch, Status};
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_image_probe_borrowed(
    input: &[u8],
    entry: &Entry,
    decode: Option<InflateScratch<'_>>,
    staging: &mut [u8],
    out: &mut ImageInfo,
) -> Status {
    crate::image_probe_borrowed::borrowed(input, entry, decode, staging, out)
}
