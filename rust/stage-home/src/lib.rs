//! Home screen layout and rendering. Code MIT; stage-backdrop.bin brand art is outside MIT (README "Name and logo"); Inter OFL-1.1.
#![no_std]
#![forbid(unsafe_code)]
mod canvas;
mod span;
mod stage;
mod words;
#[cfg(feature = "host-probe")]
pub mod host_probe;
pub use newt_core::app::AppState;
pub use newt_core::frame::{FRAME_BYTES, Frame};
pub use stage::*;
pub use words::*;
pub type StageBackdrop = [u8; 2 * FRAME_BYTES];
pub const APPROVED_BACKDROP: &StageBackdrop =
    include_bytes!("../assets/frosted-home/stage-backdrop.bin");
pub use newt_ui_canvas::HomePlane;
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeFocusStyle {
    Approved,
    #[default]
    BinaryCursor,
}
#[derive(Clone, Copy, Default)]
pub enum HomeLayout {
    #[default]
    Stage,
}
#[derive(Default)]
pub struct HomeStyle<'a> {
    pub layout: HomeLayout,
    pub focus: HomeFocusStyle,
    pub backdrop: Option<&'a StageBackdrop>,
}
/// Only the two telemetry fields Home reads for its header.
#[derive(Default)]
pub struct HeaderInfo {
    pub battery_known: bool,
    pub battery_percent: u16,
}
/// The ready cache snapshot stays borrowed through both plane paints.
#[derive(Clone, Copy)]
pub struct PreparedHomeCover<'a> {
    pub pixels: &'a [u8; 3384],
    pub ready: bool,
}
pub(crate) fn cstr(t: &[u8]) -> &[u8] {
    &t[..t.iter().position(|&v| v == 0).unwrap_or(t.len())]
}
/// Adapter to the existing validated LibraryCoverSource readAll contract.
/// CRC/cache validation stays with that source; Stage performs no storage I/O.
/// `begin` gets the caller's NUL-terminated name array unchanged, the form
/// the source validates as a NUL-terminated name.
pub trait HomeCoverSource {
    fn begin(&mut self, filename: &[u8]) -> bool;
    fn read_all(&mut self, pixels: &mut [u8; 3384]) -> bool;
    fn end(&mut self);
}
/// One full read; end runs even when filename/begin/read fails. Failure clears
/// the caller's pixels. The returned immutable snapshot borrows the full cache.
pub fn prepare_home_cover<'a>(
    source: &mut impl HomeCoverSource,
    filename: Option<&[u8]>,
    pixels: &'a mut [u8; 3384],
) -> PreparedHomeCover<'a> {
    let ready = filename
        .filter(|n| !cstr(n).is_empty())
        .is_some_and(|n| source.begin(n) && source.read_all(pixels));
    source.end();
    if !ready {
        pixels.fill(0);
    }
    PreparedHomeCover { pixels, ready }
}
#[cfg(test)]
mod tests {
    use super::*;
    use newt_core::library_name::{FILENAME_BYTES, valid_library_book_name};

    /// Checks the name the way the firmware's PreparedCovers does first.
    struct Covers;
    impl HomeCoverSource for Covers {
        fn begin(&mut self, filename: &[u8]) -> bool {
            valid_library_book_name(filename)
        }
        fn read_all(&mut self, pixels: &mut [u8; 3384]) -> bool {
            pixels.fill(0x5a);
            true
        }
        fn end(&mut self) {}
    }

    // #316: stripping the NUL made every Home cover read fail validation.
    #[test]
    fn home_cover_source_gets_the_terminated_name() {
        let mut name = [0; FILENAME_BYTES];
        name[..6].copy_from_slice(b"a.epub");
        let mut pixels = [0; 3384];
        assert!(prepare_home_cover(&mut Covers, Some(&name), &mut pixels).ready);
    }
}
