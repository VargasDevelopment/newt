#![forbid(unsafe_code)]
use newt_core::member_check::Checked;
use newt_core::{
    inflate::State,
    zip_container::{Catalog, Entry, InflateScratch, Reader, Status},
    zip_window::{arena::Arena, source::BookSource},
};
struct Borrowed<'a>(&'a [u8]);
impl BookSource for Borrowed<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else {
            return -1;
        };
        let Some(tail) = self.0.get(at..) else {
            return -1;
        };
        let n = tail.len().min(out.len());
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
// Runtime borrowed inputs prevent promotion/constant-folded empty probes.
pub fn catalog(
    input: &[u8],
    names: &mut [u8],
    entries: &mut [Entry],
    scan: &mut [u8; 4099],
    seq: &mut [u8; 1024],
    path: &[u8],
) -> (Status, usize, bool) {
    let mut source = Borrowed(input);
    let mut arena = Arena::new(names);
    let mut c = Catalog::new(&mut source, &mut arena, entries);
    let status = c.open(scan, seq);
    (status, c.entry_count(), c.find(Some(path)).is_some())
}
pub fn stream(
    input: &[u8],
    entry: &Checked,
    state: &mut State,
    dict: &mut [u8; 32768],
    buf: &mut [u8; 2048],
    out: &mut [u8],
) -> (Status, i32, u32) {
    let mut source = Borrowed(input);
    let mut r = Reader::new(
        &mut source,
        Some(InflateScratch {
            state,
            dictionary: dict,
            input: buf,
        }),
    );
    let status = r.open(entry);
    let n = if status == Status::Ok {
        r.read(out)
    } else {
        -1
    };
    (status, n, r.total_produced())
}
