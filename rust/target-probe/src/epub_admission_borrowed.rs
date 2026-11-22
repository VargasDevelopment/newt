//! Canonical RV32 inspect/chapter probes use runtime inputs and caller storage.
//! Full XML admission awaits an actual target ParserBackend.
use newt_core::{
    admission,
    zip_container::{Entry, InflateScratch, Status},
    zip_window::{arena::Arena, source::BookSource},
};
pub struct SliceSource<'a> {
    pub bytes: &'a [u8],
    pub max: usize,
}
impl BookSource for SliceSource<'_> {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else {
            return 0;
        };
        let Some(tail) = self.bytes.get(at..) else {
            return 0;
        };
        let n = tail.len().min(out.len()).min(self.max);
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
#[inline(never)]
pub fn inspect(
    bytes: &[u8],
    max: usize,
    parse_bytes: &mut [u8],
    scan: &mut [u8; 4099],
) -> (Status, usize) {
    let mut source = SliceSource { bytes, max };
    let mut scheduled = admission::ScheduledSource::new(&mut source, None, None);
    let mut parse = Arena::new(parse_bytes);
    let status = admission::inspect(&mut scheduled, &mut parse, scan);
    (status, parse.high_water())
}
#[inline(never)]
pub fn chapter(
    bytes: &[u8],
    max: usize,
    entry: &Entry,
    scratch: Option<InflateScratch<'_>>,
    out: &mut [u8],
) -> (Status, i32, bool) {
    let mut source = SliceSource { bytes, max };
    match admission::ChapterSource::open(&mut source, entry, scratch, None, None) {
        Err(s) => (s, -1, false),
        Ok(mut c) => {
            let n = c.read_at(0, out);
            let end = c.finish();
            (Status::Ok, n, end)
        }
    }
}

// Runtime borrowed data and storage; no State, dictionary or scan allocation.
#[unsafe(no_mangle)]
pub fn epub_probe_inspect(
    bytes: &[u8],
    max: usize,
    arena: &mut [u8],
    scan: &mut [u8; 4099],
) -> (Status, usize) {
    inspect(bytes, max, arena, scan)
}
#[unsafe(no_mangle)]
pub fn epub_probe_chapter(
    bytes: &[u8],
    max: usize,
    entry: &Entry,
    scratch: Option<InflateScratch<'_>>,
    out: &mut [u8],
) -> (Status, i32, bool) {
    chapter(bytes, max, entry, scratch, out)
}
