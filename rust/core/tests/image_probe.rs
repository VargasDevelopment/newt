use newt_core::image_probe::{ImageInfo, Kind, probe};
use newt_core::zip_container::{Entry, Status};
use newt_core::zip_window::source::BookSource;
struct Source<'a>(&'a [u8], usize);
impl BookSource for Source<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, dst: &mut [u8]) -> i32 {
        let tail = self.0.get(at as usize..).unwrap_or(&[]);
        let n = tail.len().min(dst.len()).min(self.1);
        dst[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
#[test]
fn output_reset_open_precedence_and_staging_admission() {
    let data = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\x02\x01\0\0\x01\x01\x08\x02";
    let mut source = Source(data, 3);
    let mut staging = [0; 1024];
    let mut out = ImageInfo {
        kind: Kind::Jpeg,
        width: 7,
        height: 8,
        progressive: true,
    };
    let mut entry = Entry::raw(26);
    assert_eq!(
        probe(&mut source, &entry, None, &mut staging, &mut out),
        Status::Ok
    );
    assert_eq!(
        out,
        ImageInfo {
            kind: Kind::Png,
            width: 513,
            height: 257,
            progressive: false
        }
    );
    entry.method = 8;
    assert_eq!(
        probe(&mut source, &entry, None, &mut [], &mut out),
        Status::Unsupported
    );
    assert_eq!(out, ImageInfo::default());
    entry.method = 0;
    assert_eq!(
        probe(&mut source, &entry, None, &mut staging[..1023], &mut out),
        Status::OutOfMemory
    );
    assert_eq!(out, ImageInfo::default());
}
#[test]
fn jpeg_pending_sof_short_length_and_zero_dimensions() {
    let mut data = [0; 26];
    data[..11].copy_from_slice(&[255, 216, 255, 194, 0, 2, 8, 0, 0, 0, 0]);
    let mut staging = [0; 1024];
    let mut out = ImageInfo::default();
    assert_eq!(
        probe(
            &mut Source(&data, 1),
            &Entry::raw(26),
            None,
            &mut staging,
            &mut out
        ),
        Status::Ok
    );
    assert_eq!(
        out,
        ImageInfo {
            kind: Kind::Jpeg,
            width: 0,
            height: 0,
            progressive: true
        }
    );
}
#[test]
fn incomplete_header_fault_is_unknown_ok() {
    let mut out = ImageInfo::default();
    let mut staging = [0; 1024];
    assert_eq!(
        probe(
            &mut Source(&[255, 216], 1),
            &Entry::raw(26),
            None,
            &mut staging,
            &mut out
        ),
        Status::Ok
    );
    assert_eq!(out, ImageInfo::default());
}
