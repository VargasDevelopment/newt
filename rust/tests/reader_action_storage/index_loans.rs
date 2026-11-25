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
struct Metadata { items:usize }
impl ParserBackend for Metadata {
    fn create(&mut self) -> bool { true }
    fn close(&mut self) {}
    fn feed(&mut self, bytes: &[u8], _: bool, handler: &mut dyn Handler) -> bool {
        if bytes.starts_with(b"<container") {
            handler.start_element(b"rootfile", &mut Attrs(&[(b"full-path", b"package.opf")]));
        } else if bytes.starts_with(b"<package") {
            handler.start_element(b"manifest", &mut Attrs(&[]));
            for i in 0..self.items {
                let id=if i==0 { "chapter".to_owned() } else { format!("id{i}") };
                handler.start_element(b"item", &mut Attrs(&[(b"id",id.as_bytes()), (b"href", b"chapter.xhtml"),
                    (b"media-type", b"application/xhtml+xml")]));
            }
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
            &mut Metadata {items:1}, pages, None, Verification::Verified).unwrap();
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
    let filename = &runtime.selection.identity().filename;
    home.with_reader_io(|scope| {
        let mut source_native = scope.card.port(scope.io);
        let mut pages_native = scope.card.port(scope.io);
        sources::with_operation(runtime.session.as_deref_mut(), runtime.source.as_mut().unwrap(), filename,
            &mut source_native, &mut pages_native, logical, &crate::Never, None,
            Probe { storage: &mut storage, lender: scope.bus.panel, digest: runtime.selection.digest(), probe }).unwrap()
    }).unwrap()
}

pub(crate) fn physical_handoffs(home:&mut app_image::Home<'_>,bytes:&[u8],digest:&[u8;32]) {
    use newt_core::{admission::{WorkEvent,WorkKind,WorkYield},book_index};
    use newt_phased_reader::phase_storage::ReadScope;
    let name=std::ffi::CString::new(std::env::args().nth(2).unwrap()).unwrap();
    let filename=name.to_str().unwrap();
    let mut path=[0;117];
    assert!(newt_core::library_catalog::library_book_path(Some(b"/newt-sd/Newt/Books\0"),Some(name.as_bytes_with_nul()),Some(&mut path)));
    let path=core::ffi::CStr::from_bytes_until_nul(&path).unwrap().to_str().unwrap();
    struct Trace<'a> {card:card::Card,cut:u8,released:Cell<bool>,path:&'a str}
    impl WorkYield for Trace<'_> {
        fn yield_now(&self) {}
        fn observe(&self,event:WorkEvent) {
            if event.kind!=WorkKind::IndexStage {return;}
            if event.result==7 {self.released.set(true);}
            if self.released.get() && matches!(event.result,5|8) {
                assert!(!self.card.0.borrow().handles.values().any(|p|p==self.path));
                // Publication takes these facts from the active UiControl and
                // reader owner, independently of whether the source FD exists.
                assert_eq!(newt_core::dev_put::admit(newt_core::dev_put::Facts {
                    home:true,reader_open:false,opening:true }),Err(newt_core::dev_put::Busy::Opening));
                assert_eq!(newt_core::dev_put::admit(newt_core::dev_put::Facts {
                    home:true,reader_open:true,opening:false }),Err(newt_core::dev_put::Busy::Reading));
                if self.cut==9 && event.result==8 {
                    self.card.0.borrow_mut().model.faults.push(crate::model::Fault::new(
                        crate::model::Op::Open,1,1,crate::model::Effect::Errno("EIO"),0));
                }
                if event.result as u8==self.cut {panic!("simulated index power cut");}
            }
        }
    }
    for cut in [0,5,8,9] {
        let old_files=home.card.0.borrow().model.files.clone();
        home.card.seed(path,bytes.to_vec());
        home.card.seed("/newt-sd/Newt/Cache/handoff-receipt",vec![1]);
        home.card.0.borrow_mut().enforce_reader_limit=true;
        let trace=Trace {card:home.card.clone(),cut,released:Cell::new(false),path};
        let caught=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            home.with_reader_io(|scope| {
                let mut source_native=scope.card.port(scope.io);
                let mut pages_native=scope.card.port(scope.io);
                let mut source=crate::physical_selected::SelectedSource::new();
                assert!(source.open(&mut source_native,path,Some(&crate::Never)));
                let receipt=source_native.open("/newt-sd/Newt/Cache/handoff-receipt",crate::Open::ReadWriteNonblock).unwrap();
                let mut key=[0;64];let key=reader_book::route_key(filename,&mut reader_hash::PackageHasher::from_live_sdk(),&mut key).unwrap();
                let route=newt_native_storage::RouteState::new(reader_book::CACHE_ROOT,key);
                let route=route.lend(&crate::Never,crate::now);
                let mut state=newt_native_storage::chapter_cache::ChapterCacheState::new();
                let mut bank=[0;newt_phased_reader::phase_storage::BANK_BYTES];let mut storage=Storage::new(&mut bank);
                let mut logical=[0;newt_core::frame::FRAME_BYTES];
                let mut pages=reader_pages::ReaderPages(newt_native_storage::chapter_cache::ChapterCacheStorage::lend(&mut state,&mut pages_native,&route).unwrap());
                let hook=&trace;
                let result=source.with_index_source(&mut source_native,name.as_bytes_with_nul(),&mut logical,Some(&crate::Never),None,|raw| {
                    let result=index_build::open_releasing(raw,digest,&mut storage,scope.bus.panel,
                        &mut Metadata {items:1500},&mut pages,Some(hook),Verification::Verified);
                    if let Ok(indexed)=&result {
                        assert!(indexed.source_reopened());
                        assert_eq!(raw.read_at(0,&mut []),0);
                    }
                    result
                });
                if cut==9 {assert!(matches!(result,Err(crate::physical_selected::SourceLoanError::BindingLost)));}
                else {result.unwrap().unwrap();}
                drop(pages);assert!(state.close(&mut pages_native,&route).unwrap().clean);
                source_native.close(receipt).unwrap();source.close(&mut source_native);
            }).unwrap();
        }));
        assert_eq!(caught.is_err(),matches!(cut,5|8));
        assert!(trace.released.get());
        {
            let mut state=home.card.0.borrow_mut();
            assert_eq!(state.most_open,3);
            if matches!(cut,5|8) {state.model.power();state.handles.clear();}
            state.enforce_reader_limit=false;
        }
        // New cold cache owner; only durable card bytes survive the cut.
        let present=home.with_reader_io(|scope| {
            let mut native=scope.card.port(scope.io);let mut key=[0;64];
            let key=reader_book::route_key(filename,&mut reader_hash::PackageHasher::from_live_sdk(),&mut key).unwrap();
            let route=newt_native_storage::RouteState::new(reader_book::CACHE_ROOT,key);
            let route=route.lend(&crate::Never,crate::now);
            let mut state=newt_native_storage::chapter_cache::ChapterCacheState::new();
            let mut pages=reader_pages::ReaderPages(newt_native_storage::chapter_cache::ChapterCacheStorage::lend(&mut state,&mut native,&route).unwrap());
            let name=*book_index::FileName::new(digest,book_index::FileKind::Index).as_bytes();
            let found=pages.file_size(&name)>=0;
            if found {
                let index=pages.query_index(&name,digest,true,&mut[0;8256],&mut||true,|store|
                    book_index::Reader::open(store,&book_index::Identity {digest:*digest,policy:newt_core::admission::POLICY_VERSION})).unwrap().0;
                assert!(index.is_some(),"cold index must be complete and valid");
            }
            drop(pages);assert!(state.close(&mut native,&route).unwrap().clean);found
        }).unwrap();
        assert_eq!(present,cut!=5);
        let mut state=home.card.0.borrow_mut();assert!(state.handles.is_empty());
        state.model.files=old_files;
        // Keep only durable fixture files; stale temp/work names from cuts are
        // intentionally exercised by the index folder migration in its tests.
    }
}

pub(crate) fn failed_verification_revokes_index(runtime:&mut Runtime,home:&mut app_image::Home<'_>,digest:&[u8;32]) {
    let session=runtime.session.as_mut().unwrap();
    let raw=runtime.source.as_mut().unwrap();
    assert!(session.verification.begin(&**raw,reader_hash::MbedHasher::default));
    raw.bytes[100]^=1;
    assert!(!session.verification.finish_with_update(&mut **raw,digest,&mut[0;4096],&mut||true,&mut|hash,bytes| {
        newt_phased_reader::imported_revision::RevisionHasher::update(hash,bytes)
    }));
    raw.bytes[100]^=1;
    home.with_reader_io(|scope| {
        let mut native=scope.card.port(scope.io);
        assert!(index_invalidation::invalidate_failed_index(session,&mut native,digest,&crate::Never,crate::now));
        assert!(session.verification.index_revoked());
        let steps=scope.card.0.borrow().model.steps;
        assert!(index_invalidation::invalidate_failed_index(session,&mut native,digest,&crate::Never,crate::now));
        assert_eq!(scope.card.0.borrow().model.steps,steps,"revoked failures do not rescan the directory on idle");
        let route=session.route.lend(&crate::Never,crate::now);
        let mut pages=reader_pages::ReaderPages(newt_native_storage::chapter_cache::ChapterCacheStorage::lend(&mut session.pages,&mut native,&route).unwrap());
        let name=*newt_core::book_index::FileName::new(digest,newt_core::book_index::FileKind::Index).as_bytes();
        assert!(pages.file_size(&name)<0,"failed verification must remove the derived index");
    }).unwrap();
}
