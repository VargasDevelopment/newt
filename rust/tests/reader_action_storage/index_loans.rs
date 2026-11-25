// SPDX-License-Identifier: MIT
//! Compare the real index loans with the operation's real route and source.
use super::*;
use crate::home_startup::CardPort;
use newt_core::{page_cache::CacheStorage, xml_session::{Attributes, Handler, ParserBackend}};
use newt_phased_reader::{index_build::{self, Indexed, Verification}, phase_storage::{BuildLoan, Storage}};


struct Attrs<'a>(&'a [(&'a [u8], &'a [u8])]);
impl Attributes for Attrs<'_> {
    fn next(&mut self) -> Option<(&[u8], &[u8])> { None }
    fn local(&self, name: &[u8]) -> Option<&[u8]> {
        self.0.iter().find(|(key, _)| *key == name).map(|(_, value)| *value)
    }
    fn exact(&self, name: &[u8]) -> Option<&[u8]> { self.local(name) }
}
// Only metadata events are faked. The index builder, source, both loan paths,
// cache adapter and card publish/read are the product implementations.
struct Metadata;
impl ParserBackend for Metadata {
    fn create(&mut self) -> bool { true }
    fn close(&mut self) {}
    fn feed(&mut self, bytes: &[u8], _: bool, handler: &mut dyn Handler) -> bool {
        if bytes.starts_with(b"<container") {
            handler.start_element(b"rootfile", &mut Attrs(&[(b"full-path", b"package.opf")]));
        } else if bytes.starts_with(b"<package") {
            handler.start_element(b"manifest", &mut Attrs(&[]));
            handler.start_element(b"item", &mut Attrs(&[(b"id", b"chapter"), (b"href", b"chapter.xhtml"),
                (b"media-type", b"application/xhtml+xml")]));
            handler.end_element(b"manifest");
            handler.start_element(b"spine", &mut Attrs(&[]));
            handler.start_element(b"itemref", &mut Attrs(&[(b"idref", b"chapter")]));
            handler.end_element(b"spine");
        } else { assert!(bytes.is_empty(), "unexpected metadata input"); }
        true
    }
}
struct Probe<'a, 'bank, L> {
    storage: &'a mut Storage<'bank>, lender: &'a mut L, digest: &'a [u8; 32], probe: &'a [u8],
}
impl<N: Native + newt_native_storage::page_entries::PageEntries, L: BuildLoan>
    sources::Operation<N, Indexed> for Probe<'_, '_, L> {
    fn run(self, pages: &mut reader_pages::ReaderPages<'_, N>, source: sources::Source<'_>) -> Result<Indexed, BookFailure> {
        let raw = match source {
            sources::Source::Strict(raw) => raw,
            sources::Source::Session(raw, verification) => {
                assert_eq!(verification.state(), crate::reader_verification::Verification::Verified);
                raw
            }
        };
        let indexed = index_build::open(raw, self.digest, self.storage, self.lender,
            &mut Metadata, pages, None, Verification::Verified).unwrap();
        let mut probe = [0; 17];
        assert_eq!(raw.read_at(0, &mut probe), 17);
        assert!(pages.begin_write(self.probe) && pages.write(&probe) && pages.end_write());
        Ok(indexed)
    }
}
pub(crate) fn index(runtime: &mut Runtime, home: &mut app_image::Home<'_>, logical: &mut newt_core::frame::Frame) -> Indexed {
    let probe: &[u8] = if runtime.session.is_some() { b"session-loan-probe" } else { b"standalone-loan-probe" };
    runtime.memory.probe = probe;
    let mut bank = [0; newt_phased_reader::phase_storage::BANK_BYTES];
    let mut storage = Storage::new(&mut bank);
    let filename = newt_core::library_name::name_bytes(&runtime.selection.identity().filename).unwrap();
    let filename = core::str::from_utf8(filename).unwrap();
    home.with_reader_io(|scope| {
        let mut source_native = scope.card.port(scope.io);
        let mut pages_native = scope.card.port(scope.io);
        sources::with_operation(runtime.session.as_deref_mut(), runtime.source.as_mut().unwrap(), filename,
            &mut source_native, &mut pages_native, logical, &crate::Never, None,
            Probe { storage: &mut storage, lender: scope.bus.panel, digest: runtime.selection.digest(), probe }).unwrap()
    }).unwrap()
}
