//! Synthetic metadata events for the bounded builder's fault/card/limit models.
use super::core_api::{book_index::{self, Identity, Member}, book_index_build::{Builder, Cursor, CURSORS, Error, JoinState, POSITION_WORDS, Prepared, Progress, WRITER_PREFIX, WorkFile},
    member_check::Checked, package_parsers::Workflow, xml_session::{Attributes, Handler, ParserBackend},
    zip_container::Status, zip_window::source::BookSource};
pub const IDENTITY: Identity = Identity { digest: [0xa5; 32], policy: 1 };
extern crate std;
use self::std::format;
struct Source;
impl BookSource for Source {
    fn size(&self) -> u64 { 1 }
    fn read_at(&mut self, _: u64, out: &mut [u8]) -> i32 { out[0] = b'x'; 1 }
}
struct Attrs<'a>(&'a [(&'a [u8], &'a [u8])]);
impl Attributes for Attrs<'_> {
    fn next(&mut self) -> Option<(&[u8], &[u8])> { None }
    fn local(&self, name: &[u8]) -> Option<&[u8]> { self.0.iter().find(|(key, _)| *key == name).map(|(_, value)| *value) }
    fn exact(&self, name: &[u8]) -> Option<&[u8]> { self.local(name) }
}
struct Backend { manifests: u32, spine: u32 }
impl ParserBackend for Backend {
    fn create(&mut self) -> bool { true }
    fn close(&mut self) {}
    fn feed(&mut self, bytes: &[u8], _: bool, h: &mut dyn Handler) -> bool {
        if bytes.is_empty() { return true; }
        let mut empty = Attrs(&[]);
        h.start_element(b"manifest", &mut empty);
        for i in 0..self.manifests {
            let id = format!("i{i}");
            let href = format!("c{i}");
            h.start_element(b"item", &mut Attrs(&[(b"id", id.as_bytes()), (b"href", href.as_bytes()), (b"media-type", b"application/xhtml+xml")]));
        }
        h.end_element(b"manifest");
        h.start_element(b"spine", &mut empty);
        for i in 0..self.spine {
            let id = format!("i{i}");
            h.start_element(b"itemref", &mut Attrs(&[(b"idref", id.as_bytes())]));
        }
        h.end_element(b"spine");
        true
    }
}
/// Host backing for the merge state the device lends from its parse bank.
pub struct JoinBacking { positions: [u64; POSITION_WORDS], cursors: [Cursor; CURSORS], saved: [u8; WRITER_PREFIX] }
impl JoinBacking {
    pub fn new() -> Self { Self { positions: [0; POSITION_WORDS], cursors: [Cursor::default(); CURSORS], saved: [0; WRITER_PREFIX] } }
    pub fn state(&mut self) -> JoinState<'_> { JoinState::new(&mut self.positions, &mut self.cursors, &mut self.saved) }
}
pub fn prepare<'a>(work: &mut impl WorkFile, buffer: &'a mut [u8], run_bytes: usize,
    members: u32, manifests: u32, spine: u32, state: &mut JoinState<'_>) -> Result<Prepared<'a>, Error> {
    prepare_watched(work, buffer, run_bytes, members, manifests, spine, state, &mut |_| true)
}
#[allow(clippy::too_many_arguments)]
pub fn prepare_watched<'a>(work: &mut impl WorkFile, buffer: &'a mut [u8], run_bytes: usize, members: u32,
    manifests: u32, spine: u32, state: &mut JoinState<'_>, progress: &mut dyn FnMut(Progress) -> bool) -> Result<Prepared<'a>, Error> {
    let min_read = if buffer.len() == 32768 { 512 } else { 32 };
    let mut builder = Builder::new(buffer, min_read, run_bytes)?;
    for i in 0..members {
        let status = builder.visit(work, Member { hash: book_index::name_hash(format!("c{i}").as_bytes()),
            local_offset: i * 64, compressed: 10, uncompressed: 10, method: 0, flags: 0, contents_spine: book_index::NONE }, &mut *progress);
        if status != Status::Ok { return Err(Error::Book(status)); }
    }
    let (mut source, mut backend, mut input) = (Source, Backend { manifests, spine }, [0; 2048]);
    let workflow = Workflow { source: &mut source, backend: &mut backend, decoder: None,
        buffer: Some(&mut input), filter: None, policy: None };
    builder.parse(workflow, &Checked::raw(1), b"", IDENTITY, work, &mut *progress)?
        .join(work, state, progress)
}
