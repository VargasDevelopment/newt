// SPDX-License-Identifier: MIT
//! Actual operate_image wiring and revision/receipt/source loans. Board, card,
//! input and the RunLoan consumer are host ports; this is not a layout test.
extern crate alloc;
use core::{cell::Cell, ffi::CStr};
use std::{marker::PhantomData, rc::Rc, cell::RefCell};
pub use newt_native_storage::{Directory, Error, Kind, Native, Open, Result, Stat};
#[path = "../../firmware/board_clock.rs"] pub mod board_clock;
#[path = "../../firmware/home_startup.rs"] mod actual_home;
pub mod home_startup {
    pub use crate::actual_home::*;
    pub type FirmwareCard = crate::card::Card;
}
#[path = "../../firmware/nvs/nvs.rs"] pub mod nvs;
#[path = "../../firmware/reader_book.rs"] pub mod reader_book;
#[path = "../../firmware/reader_metadata.rs"] pub mod reader_metadata;
#[path = "../../firmware/reader_pages.rs"] pub mod reader_pages;
#[path = "../../firmware/reading_state_medium.rs"] pub mod reading_state_medium;
#[path = "../../firmware/sd_revision.rs"] pub mod sd_revision;
#[path = "../../firmware/stats_clock.rs"] pub mod stats_clock;
#[path = "../../firmware/reader_exit_metadata.rs"] mod reader_exit_metadata;
#[path = "../reader_checkpoint/card.rs"] mod card;
#[path = "../sd_reading_state/model.rs"] mod model;
use home_startup::CardPort;
use newt_core::{progress::Progress, selection_authority::SelectionAuthority};
use newt_phased_reader::imported_revision::{Digest, ImportedRevision, RevisionHasher, RevisionPorts};
use newt_core::zip_window::source::BookSource;
use newt_reader_ui::{ReaderSource, ReaderSourceState, ViewSettings};
use reader_book::{BookFailure, ReaderCheckpoint, ReadingStatsRuntime};
use newt_sdk_heap::Fixed;

#[derive(Default)] pub struct Log;
impl core::fmt::Write for Log { fn write_str(&mut self, _: &str) -> core::fmt::Result { Ok(()) } }
pub struct Flash;
impl nvs::Native for Flash {
    fn storage_ready(&mut self) -> bool { false }
    fn open(&mut self, _: &CStr, _: bool, _: &mut u32) -> core::result::Result<(), nvs::NativeError> { Err(nvs::NativeError::NOT_FOUND) }
    fn close(&mut self, _: u32) {}
    fn get_blob(&mut self, _: u32, _: &CStr, _: Option<&mut [u8]>, _: &mut usize) -> core::result::Result<(), nvs::NativeError> { Err(nvs::NativeError::NOT_FOUND) }
    fn set_blob(&mut self, _: u32, _: &CStr, _: &[u8]) -> core::result::Result<(), nvs::NativeError> { panic!("unexpected NVS write") }
    fn commit(&mut self, _: u32) -> core::result::Result<(), nvs::NativeError> { panic!("unexpected NVS commit") }
    fn erase_key(&mut self, _: u32, _: &CStr) -> core::result::Result<(), nvs::NativeError> { panic!("unexpected NVS erase") }
    fn readback_durable(&self) -> bool { true }
}
mod sdk { pub type Sdk552 = crate::Flash; }
mod app_power { pub fn delay_ms(_: u32) { panic!("no input wait in this source-loan fixture") } }
mod reader_action {
    pub use newt_core::reader_action_profile::ActionStage;
    #[derive(Clone, Copy)] pub struct Trace;
}
mod app_image {
    use super::*;
    pub type Home<'sdk> = home_startup::HomeRuntime<'sdk, Log, Flash, card::Card>;
    pub struct Application<'a> {
        pub info: newt_reader_ui::DeviceInfo<'static>, pub app: newt_core::app::AppState,
        pub displayed: Displayed, pub last_battery_read: u32, pub loan: PhantomData<&'a ()>,
        pub hardware: newt_c3_board::hardware::HardwareReader<'a>,
    }
    pub struct Displayed { pub valid: bool }
    impl Application<'_> {
        pub fn read_info_with_clock(&mut self, _: &newt_c3_board::MonotonicClock, _: reader_action::Trace) -> core::result::Result<(), ()> { panic!("no battery work in source-loan fixture") }
    }
    pub static TURN_PENDING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    pub struct Queue;
    impl Queue {
        pub fn pending(&self) -> bool { TURN_PENDING.load(core::sync::atomic::Ordering::Relaxed) }
    }
    pub struct Events<'a, 'task> {
        pub control: &'a mut (), pub queue: &'a Queue, pub action: reader_action::Trace,
        pub task: PhantomData<&'task ()>,
    }
}
mod reader_duty {
    use super::*;
    pub struct ReaderDuty<'a, 'task>(PhantomData<(&'a (), &'task ())>);
    impl<'a, 'task> ReaderDuty<'a, 'task> {
        pub fn new(_: &'a (), _: &'a newt_core::reader_scheduler::YieldCadence,
            _: &'a newt_c3_board::MonotonicClock, _: Option<&'a newt_core::reader_work_profile::ReaderWorkProfile>, _: reader_action::Trace) -> Self { Self(PhantomData) }
        pub fn cancelled(&self) -> bool { false }
        pub fn action_boundary(&self, _: u8) {}
        pub fn open_route(&self, _: u32) {}
        pub fn cache_miss(&self, _: u8) {}
        pub fn yield_now(&self) {}
        pub fn event(&self, _: &newt_core::admission::WorkEvent) {}
    }
}
mod reader_hash {
    use super::*;
    #[link(name = "crypto")]
    unsafe extern "C" { fn SHA256(data: *const u8, length: usize, out: *mut u8) -> *mut u8; }
    pub trait SourceHashObserver {}
    pub fn package_hash(input: &[u8], out: &mut Digest) -> bool {
        // SAFETY: the existing host libcrypto boundary reads this slice and fills 32 bytes.
        !unsafe { SHA256(input.as_ptr(), input.len(), out.as_mut_ptr()) }.is_null()
    }
    #[derive(Default)] pub struct MbedHasher(Vec<u8>);
    pub struct Loan<'a>(&'a mut MbedHasher);
    impl MbedHasher { pub unsafe fn new(_: Option<&dyn Fn()>) -> Self { Self::default() } pub fn borrow_with_yield(&mut self, _: &dyn Fn()) -> Loan<'_> { Loan(self) } }
    impl RevisionHasher for MbedHasher {
        fn begin(&mut self) -> bool { self.0.clear(); true }
        fn update(&mut self, input: &[u8]) -> bool { self.0.extend_from_slice(input); true }
        fn finish(&mut self, out: &mut Digest) -> bool { package_hash(&self.0, out) }
        fn hash(&mut self, input: &[u8], out: &mut Digest) -> bool { package_hash(input, out) }
    }
    impl RevisionHasher for Loan<'_> {
        fn begin(&mut self) -> bool { self.0.begin() }
        fn update(&mut self, input: &[u8]) -> bool { self.0.update(input) }
        fn finish(&mut self, out: &mut Digest) -> bool { self.0.finish(out) }
        fn hash(&mut self, input: &[u8], out: &mut Digest) -> bool { self.0.hash(input, out) }
    }
    pub struct PackageHasher;
    impl PackageHasher { pub fn from_live_sdk() -> Self { Self } }
    impl newt_phased_reader::hash_provider::CacheHash for PackageHasher {
        fn hash(&mut self, input: &[u8], out: &mut Digest) -> bool { package_hash(input, out) }
    }
    pub unsafe fn hash_source(source: &mut dyn BookSource, digest: &mut Digest, _: &mut dyn FnMut(), _: Option<&dyn SourceHashObserver>) -> bool {
        let mut bytes=vec![0;source.size() as usize];
        let mut at=0;
        while at<bytes.len() {
            let n=source.read_at(at as u64,&mut bytes[at..]);
            if n<=0 {return false;}
            at+=n as usize;
        }
        package_hash(&bytes,digest)
    }
}
#[path = "../../firmware/reader_verification.rs"] mod reader_verification;
#[path = "../../firmware/reader_selected_source.rs"] mod physical_selected;
mod selected {
    use super::*;
    pub trait FileObserver {}
    pub struct SelectedSource { pub bytes: Vec<u8>, pub reads: Rc<RefCell<Vec<usize>>> }
    impl BookSource for SelectedSource {
        fn size(&self) -> u64 { self.bytes.len() as u64 }
        fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
            if !out.is_empty() { self.reads.borrow_mut().push(out.len()); }
            let Some(bytes) = self.bytes.get(at as usize..) else { return -1; };
            let n = bytes.len().min(out.len()); out[..n].copy_from_slice(&bytes[..n]); n as i32
        }
    }
    // Bytes stay in memory, so a spilled index build can release and reopen.
    impl newt_phased_reader::index_build::ReleasableSource for SelectedSource {
        fn release(&mut self) -> bool { true }
        fn reopen(&mut self) -> bool { true }
    }
    impl SelectedSource {
        pub fn with_index_source<N,R>(&mut self,_:&mut N,_:&[u8],_:&mut[u8],_:Option<&dyn newt_native_storage::Authority>,_:Option<&dyn FileObserver>,work:impl FnOnce(&mut dyn newt_phased_reader::index_build::ReleasableSource)->R)->core::result::Result<R,()> {Ok(work(self))}
        pub fn bound(&self) -> bool { true }
        pub fn with_source_observed<N, R>(&mut self, _: &mut N, _: Option<&dyn newt_native_storage::Authority>, _: Option<&dyn FileObserver>, work: impl FnOnce(&mut dyn BookSource) -> R) -> core::result::Result<R, ()> { Ok(work(self)) }
    }
}
mod image {
    use crate::{app_image, card, home_startup, nvs, reader_action, reader_book,
        reader_exit_metadata, reader_hash, reader_pages, sd_revision, selected, Flash, Log, Native};
    use core::{cell::Cell, ffi::CStr};
    use newt_core::{progress::Progress, selection_authority::SelectionAuthority};
    use newt_phased_reader::imported_revision::Digest;
    use newt_core::zip_window::source::BookSource;
    use newt_reader_ui::{ReaderSourceState, ViewSettings};
    use reader_book::{BookFailure, ReaderCheckpoint, ReadingStatsRuntime};
    use newt_sdk_heap::Fixed;
    pub mod profile {
        use super::*;
        pub struct Observations<'a> { pub profile: &'a newt_core::reader_work_profile::ReaderWorkProfile, pub clock: newt_c3_board::MonotonicClock }
        impl selected::FileObserver for Observations<'_> {}
        impl reader_hash::SourceHashObserver for Observations<'_> {}
    }
    pub mod producer {
        use super::*;
        pub struct Poll<'a, 'task> {
            pub duty: &'a crate::reader_duty::ReaderDuty<'a, 'task>, pub clock: newt_c3_board::MonotonicClock,
            pub queue: &'a app_image::Queue, pub console: &'a Log, pub view_valid: bool, pub card_missing: bool,
            pub power: &'a Cell<bool>, pub progress: &'a Cell<u32>, pub battery: &'a Cell<u32>, pub began: Cell<u32>,
        }
        impl Poll<'_, '_> {
            pub fn entry(&self, _: u32) -> reader_book::ProducerEntry { reader_book::ProducerEntry::Blocked }
            pub fn stop(&self, _: u32) -> newt_core::producer_policy::Stop { newt_core::producer_policy::Stop::Terminal }
            pub fn pause_request(&self, _: u32, _: u8) -> Option<newt_core::producer_policy::Stop> { Some(newt_core::producer_policy::Stop::Terminal) }
            pub fn yield_now(&self) {}
            pub fn after_duty(&self, _: bool, _: bool) -> newt_core::producer_policy::Stop { newt_core::producer_policy::Stop::Terminal }
        }
    }
    pub mod session {
        use super::*;
        #[path = "../../firmware/reader_image_source_loan.rs"] mod actual_loan;
        pub(crate) use actual_loan::{with_source, with_source_lent};
        pub type Verification = crate::reader_verification::ReaderVerification<reader_hash::MbedHasher>;
        pub struct Session {
            pub route: newt_native_storage::RouteState, pub receipt: sd_revision::SdRevisionState,
            pub pages: newt_native_storage::chapter_cache::ChapterCacheState,
            pub whole: reader_hash::MbedHasher, pub block: reader_hash::MbedHasher, pub scratch: [u8;256],
            pub source: Option<ReaderSourceState>, pub verification: Verification,
        }
    }
    pub struct Memory { pub runtime: newt_c3_board::reader_memory::Runtime<'static,'static>, pub probe: &'static [u8] }
    impl Memory {
        pub fn external_mut(&mut self) -> Option<&mut Self> { Some(self) }
        pub fn lend<R>(&mut self, work: impl FnOnce(&mut Self) -> R) -> R { work(self) }
    }
    pub struct Saves;
    impl Saves { pub fn checkpoint<T>(&mut self, _: T) {} }
    pub struct Idle;
    impl Idle {
        pub fn note_input(&mut self, _: u32) { panic!("no checkpoint duty in source-loan fixture") }
    }
    pub mod idle_save {
        use super::*;
        pub fn checkpoint(
            _: reader_book::ProducerPause<'_, '_, '_, '_, Flash, card::Card>,
            _: &mut Idle, _: &newt_c3_board::MonotonicClock,
            _: &mut newt_c3_board::hardware::HardwareReader<'_>,
            _: newt_core::time_zone::TimeZone, _: &Cell<u32>, _: &mut bool,
            _: &dyn Fn() -> bool,
        ) -> (newt_core::producer_policy::Stop, bool) {
            panic!("no checkpoint duty in source-loan fixture")
        }
    }
    pub struct Runtime {
        pub memory: Memory, pub reader_ui: newt_reader_ui::ReaderUiState, pub selection: SelectionAuthority,
        pub saved: Progress, pub title: &'static mut [u8;256], pub home: &'static mut newt_device_ui::HomeData<'static>,
        pub source: Option<Box<selected::SelectedSource>>, pub session: Option<Box<session::Session>>,
        pub checkpoint: ReaderCheckpoint, pub statistics: ReadingStatsRuntime, pub recovery: Option<reader_book::PendingRecovery>,
        pub cadence: newt_core::reader_scheduler::YieldCadence, pub settings: ViewSettings,
        pub exit_metadata: Option<Fixed<reader_exit_metadata::ExitMetadata>>, pub last_progress: Cell<u32>, pub saves: Saves, pub idle: Idle, pub index_trim_owed: bool,
    }
    impl Runtime {
        pub fn book(&self) -> Option<&newt_c3_board::reader_memory::Runtime<'static,'static>> { Some(&self.memory.runtime) }
        pub fn restore_policy_image(&self) -> newt_reader_ui::RestorePolicy { newt_reader_ui::RestorePolicy::ExactPage }
        pub fn refuse_checkpoint_image(&mut self, _: u32, _: bool) { panic!("no requested save in source-loan fixture") }
    }
    #[path = "../../firmware/reader_standalone_open.rs"] pub(crate) mod standalone_open;
    #[path = "../../firmware/reader_index_reopen.rs"] pub(crate) mod index_reopen;
    #[path = "../../firmware/reader_index_invalidation.rs"] pub(crate) mod index_invalidation;
    pub mod drive {
        use super::*;
        pub struct Opening { pub accepted_save: bool, pub at: u32, pub indexed: newt_phased_reader::index_build::Indexed }
        // Whether the last turn opened through the prepared import route.
        thread_local! { pub static PREPARED: Cell<Option<bool>> = const { Cell::new(None) }; }
        #[derive(Clone, Copy)] pub struct Turn<'a> { pub indexed: Option<&'a newt_phased_reader::index_build::Indexed>, pub paint_first: bool, pub accepted_save: bool, pub step: Option<reader_book::BookStep>, pub settings: ViewSettings, pub open: newt_reader_ui::PhysicalOpen }
        pub struct Ports<'a,'io,'scope,'sdk> {
            pub io: &'io newt_native_io::UiIo, pub panel: &'a mut newt_c3_board::ReaderPanel<'scope,'sdk>,
            pub card: &'a mut card::Card, pub preferences: &'a mut home_startup::preferences::ReaderPreferences,
            pub nvs: &'a mut Flash, pub facts: &'a mut newt_native_storage::reading_state::StateFacts,
        }
        pub struct RunLoan<'a> {
            pub memory: &'a mut Memory, pub ui: &'a mut newt_reader_ui::ReaderUiState,
            pub checkpoint: &'a mut ReaderCheckpoint, pub statistics: &'a mut ReadingStatsRuntime,
            pub recovery: &'a mut Option<reader_book::PendingRecovery>, pub accepted: &'a mut bool,
            pub metadata: &'a mut Option<Fixed<reader_exit_metadata::ExitMetadata>>, pub flags: Option<&'a mut u16>, pub settings: &'a mut ViewSettings,
        }
        impl RunLoan<'_> {
            pub fn run<'io>(self, _: Ports<'_,'io,'_,'_>, pages: &mut reader_pages::ReaderPages<'_, impl Native>,
                source: &mut impl newt_reader_ui::SourceOperations, request: &reader_book::BookRequest<'_>, _: &reader_book::ProducerCallbacks<'_>,
                _: &mut reader_book::ProducerPauseFn<'_,'io,Flash,card::Card>, _: Option<(&mut [u8;256],&mut u16)>, turn: Turn<'_>,
                _: &dyn Fn(), _: &dyn Fn(newt_core::admission::WorkEvent),
                _: &mut Saves, _: bool, _: &mut bool,
            ) -> core::result::Result<reader_book::BookExit,BookFailure> {
                PREPARED.with(|prepared| prepared.set(Some(matches!(turn.open, newt_reader_ui::PhysicalOpen::Prepared))));
                if matches!(turn.step, Some(reader_book::BookStep::Idle { .. })) {
                    use core::sync::atomic::Ordering::Relaxed;
                    assert!(!request.cancel.cancelled());
                    // Inject a turn after idle admission, as while census is walking.
                    app_image::TURN_PENDING.store(true, Relaxed);
                    assert!(request.cancel.cancelled(), "a queued turn must cancel the idle trim authority");
                    app_image::TURN_PENDING.store(false, Relaxed);
                    assert!(!request.cancel.cancelled());
                }
                let mut first = [0;4096]; let mut second = [0;4096];
                assert_eq!(source.read_at(0,&mut first),4096); assert_eq!(source.read_at(0,&mut second),4096);
                assert_eq!(first,second);
                let mut probe = [0;17];
                assert_eq!(newt_core::page_cache::CacheStorage::read_at(pages,self.memory.probe,0,&mut probe),17,
                    "index and operate_image must lend the same cache route");
                assert_eq!(probe,first[..17],"index and operate_image must lend the same selected bytes");
                // Typed stop after consumption; the actual guard must restore
                // source state even when its consumer returns an error.
                Err(BookFailure::Open(newt_core::zip_container::Status::Unsupported))
            }
        }
    }
    fn millis() -> u32 { u32::MAX }
    #[path = "../../firmware/reader_image_sources.rs"] mod sources;
    #[path = "../../firmware/reader_image_operation.rs"] mod operation;
    #[path = "../reader_action_storage/index_loans.rs"] pub(crate) mod index_loans;
    pub fn execute_open(runtime: &mut Runtime, home: &mut app_image::Home<'_>, ui: &mut app_image::Application<'_>, logical: &mut newt_core::frame::Frame, events: &mut app_image::Events<'_,'_>, indexed: newt_phased_reader::index_build::Indexed) {
        let opening=drive::Opening { accepted_save:false,at:0,indexed };
        assert!(matches!(runtime.operate_image(home,ui,logical,events,Some(opening),None,false,None,None), Err(BookFailure::Open(newt_core::zip_container::Status::Unsupported))));
    }
    pub fn execute(runtime: &mut Runtime, home: &mut app_image::Home<'_>, ui: &mut app_image::Application<'_>, logical: &mut newt_core::frame::Frame, events: &mut app_image::Events<'_,'_>, step: reader_book::BookStep) {
        assert!(matches!(runtime.operate_image(home,ui,logical,events,None,Some(step),false,None,None), Err(BookFailure::Open(newt_core::zip_container::Status::Unsupported))));
    }
}

struct Never;
impl newt_native_storage::Authority for Never { fn cancelled(&self) -> bool { false } }
fn now() -> u32 { u32::MAX }
fn main() {
    let mut tokens = [0u8;2];
    // SAFETY: the existing Board host fake receives two disjoint tokens.
    let (board,pending,_) = unsafe { newt_c3_board::Board::acquire_x3_optional_sd(
        (&mut tokens[0] as *mut u8).cast(),(&mut tokens[1] as *mut u8).cast()) }.unwrap();
    drop(pending);
    let hardware = board.hardware_reader();
    let card = card::Card::default();
    card.seed("/newt-sd/Newt/Cache/.keep", vec![]);
    let mut home = home_startup::HomeRuntime::restore_durable(board_clock::BoardBus { board, log: Log },Flash,card);
    let bytes = std::fs::read(std::env::args().nth(1).expect("generated EPUB")).unwrap();
    let reads = Rc::new(RefCell::new(Vec::new()));
    let mut source = Box::new(selected::SelectedSource { bytes, reads:reads.clone() });
    let mut digest = [0;32]; assert!(reader_hash::package_hash(&source.bytes,&mut digest));
    assert_eq!(image::standalone_open::verify(&mut *source,Some(digest),&|| {},&||true,None).unwrap(),digest);
    source.bytes[100]^=1;
    assert!(matches!(image::standalone_open::verify(&mut *source,Some(digest),&|| {},&||true,None),
        Err(BookFailure::Open(newt_core::zip_container::Status::Stale))),
        "strict prepare hash must reject changed bytes");
    source.bytes[100]^=1;
    image::index_loans::physical_handoffs(&mut home,&source.bytes,&digest);
    let filename = std::env::args().nth(2).expect("catalog name");
    let filename_for_spill = filename.clone();
    let mut key = [0;64];
    let key = reader_book::route_key(&filename,&mut reader_hash::PackageHasher::from_live_sdk(),&mut key).unwrap();
    let route = newt_native_storage::RouteState::new(reader_book::CACHE_ROOT,key);
    let receipt = home.with_reader_io(|scope| {
        let mut native = scope.card.port(scope.io);
        sd_revision::SdRevisionState::new(&mut native,&route.lend(&Never,now))
    }).unwrap();
    let mut session = Box::new(image::session::Session {
        route, receipt, pages:newt_native_storage::chapter_cache::ChapterCacheState::new(),
        whole:reader_hash::MbedHasher::default(), block:reader_hash::MbedHasher::default(), scratch:[0;256], source:None,
        verification:image::session::Verification::new(),
    });
    home.with_reader_io(|scope| {
        let mut native = scope.card.port(scope.io);
        let route = session.route.lend(&Never,now);
        let mut receipt = sd_revision::SdRevisionStorage::lend(&mut session.receipt,&mut native,&route).unwrap();
        let mut revision = ImportedRevision::compact(&mut receipt,&mut session.whole,&mut session.block,&mut session.scratch);
        assert!(revision.begin_import(&mut *source));
        session.source = Some(ReaderSource::new(revision).into_state());
    }).unwrap();
    reads.borrow_mut().clear();
    let selections=Box::leak(Box::new([newt_phased_reader::Selection::default(),newt_phased_reader::Selection::default()]));
    let runs=Box::leak(Box::new([newt_phased_sink::Run::default();newt_phased_sink::RUNS]));
    let text=Box::leak(Box::new([0;newt_phased_sink::TEXT]));
    let plan=Box::leak(Box::new(newt_phased_sink::Plan::new(runs,text)));
    let mut selection = SelectionAuthority::new();
    assert!(selection.publish_candidate(&std::ffi::CString::new(filename).unwrap(),digest,&mut || {},&mut || {}));
    let mut runtime=image::Runtime {
        memory:image::Memory { runtime:newt_c3_board::reader_memory::Runtime::new(selections,plan), probe:b"unwritten" },
        reader_ui:newt_reader_ui::ReaderUiState::default(), selection,
        saved:Progress::default(), title:Box::leak(Box::new([0;256])), home:Box::leak(Box::new(newt_device_ui::HomeData::default())),
        source:Some(source), session:Some(session), checkpoint:ReaderCheckpoint::new(reader_book::CheckpointRestore::Latest),
        statistics:ReadingStatsRuntime::default(), recovery:None, cadence:newt_core::reader_scheduler::YieldCadence::new(),
        settings:ViewSettings::default(), exit_metadata:None, last_progress:Cell::new(u32::MAX), saves:image::Saves, idle:image::Idle, index_trim_owed:false,
    };
    let mut ui=app_image::Application {
        info:newt_reader_ui::DeviceInfo::default(), app:newt_core::app::AppState::default(),
        displayed:app_image::Displayed { valid:false }, last_battery_read:u32::MAX, loan:PhantomData,
        hardware,
    };
    let mut control=();let queue=app_image::Queue;
    let mut events=app_image::Events { control:&mut control,queue:&queue,action:reader_action::Trace,task:PhantomData };
    let mut logical=[0xa5;newt_core::frame::FRAME_BYTES];
    image::index_loans::index(&mut runtime,&mut home,&mut logical,false);
    assert!(runtime.session.as_ref().unwrap().source.is_some(),"index loans retain the operation's revision owner");
    logical.fill(0xa5);
    reads.borrow_mut().clear();
    image::execute(&mut runtime,&mut home,&mut ui,&mut logical,&mut events,reader_book::BookStep::Action(newt_core::app::Action::Next));
    assert_eq!(&*reads.borrow(),&[4096],"real operate_image must lend whole-block action scratch exactly once");
    let retained_at = newt_phased_reader::imported_revision::BLOCK_BYTES
        + newt_phased_reader::imported_revision::RETAINED_HEADER_BYTES;
    assert!(&logical[retained_at..retained_at + 4096] == &runtime.source.as_ref().unwrap().bytes[..4096], "the caller's existing logical frame is the authenticated block backing");
    logical.fill(0x7d);
    reads.borrow_mut().clear();
    let session=runtime.session.as_mut().unwrap();let raw=runtime.source.as_mut().unwrap();
    home.with_reader_io(|scope| {
        let mut native=scope.card.port(scope.io);let route=session.route.lend(&Never,now);
        let mut receipt=sd_revision::SdRevisionStorage::lend(&mut session.receipt,&mut native,&route).unwrap();
        assert!(image::session::with_source(&mut session.source,RevisionPorts {
            source:Some(&mut **raw),store:&mut receipt,whole:&mut session.whole,block:Some(&mut session.block),scratch:&mut session.scratch,
        },|source| { let mut out=[0;17];assert_eq!(source.read_at(0,&mut out),17); }).is_ok(),"actual operation restored the original 256-byte source binding");
    }).unwrap();
    assert_eq!(reads.borrow().as_slice(),&[256;16],"next action authenticates anew using original compact scratch");
    assert!(logical.iter().all(|b|*b==0x7d),"withdrawn source cannot touch the returned frame");
    reads.borrow_mut().clear();
    image::execute(&mut runtime,&mut home,&mut ui,&mut logical,&mut events,
        reader_book::BookStep::Idle { checkpoint:false, admitted:false });
    assert_eq!(reads.borrow().as_slice(),&[256;32],"idle keeps compact scratch");
    assert!(logical.iter().all(|b|*b==0x7d),"idle must preserve the published logical frame");
    image::index_loans::failed_verification_revokes_index(&mut runtime,&mut home,&digest);
    // Distinct probe names prevent a session record from hiding standalone
    // route drift. Return the session's descriptor before withdrawing its owner.
    let mut session=runtime.session.take().unwrap();
    home.with_reader_io(|scope| {
        let mut native=scope.card.port(scope.io);let route=session.route.lend(&Never,now);
        assert!(session.pages.close(&mut native,&route).unwrap().clean);
    }).unwrap();
    drop(session);
    let indexed=image::index_loans::index(&mut runtime,&mut home,&mut logical,false);
    logical.fill(0xa5);
    reads.borrow_mut().clear();
    image::execute_open(&mut runtime,&mut home,&mut ui,&mut logical,&mut events,indexed);
    assert_eq!(reads.borrow().as_slice(),&[4096,4096],"standalone operation uses the source seen by the index pass");
    println!("PASS whole actual operate_image: caller frame loan, whole-block retention and original-scratch withdrawal; RunLoan is a typed host consumer");
    spill_reopened_first_open(&mut runtime,&mut home,&mut ui,&mut logical,&mut events,&filename_for_spill,&digest);
}

// #171: a first open whose index build spills reopens the source, which
// restarts verification. The provisional import must still open Prepared.
// A pending open that is not prepared (the hint path) keeps Selected.
fn spill_reopened_first_open(runtime:&mut image::Runtime,home:&mut app_image::Home<'_>,ui:&mut app_image::Application<'_>,
    logical:&mut newt_core::frame::Frame,events:&mut app_image::Events<'_,'_>,filename:&str,digest:&[u8;32]) {
    use crate::reader_verification::Verification;
    let mut key=[0;64];
    let key=reader_book::route_key(filename,&mut reader_hash::PackageHasher::from_live_sdk(),&mut key).unwrap();
    let route=newt_native_storage::RouteState::new(reader_book::CACHE_ROOT,key);
    let receipt=home.with_reader_io(|scope| {
        let mut native=scope.card.port(scope.io);
        sd_revision::SdRevisionState::new(&mut native,&route.lend(&Never,now))
    }).unwrap();
    let mut session=Box::new(image::session::Session {
        route, receipt, pages:newt_native_storage::chapter_cache::ChapterCacheState::new(),
        whole:reader_hash::MbedHasher::default(), block:reader_hash::MbedHasher::default(), scratch:[0;256], source:None,
        verification:image::session::Verification::new(),
    });
    let source=runtime.source.as_mut().unwrap();
    home.with_reader_io(|scope| {
        let mut native=scope.card.port(scope.io);
        let route=session.route.lend(&Never,now);
        // Cold first open: no index for this digest yet.
        let mut pages=reader_pages::ReaderPages(newt_native_storage::chapter_cache::ChapterCacheStorage::lend(&mut session.pages,&mut native,&route).unwrap());
        pages.0.invalidate_index(digest);
        let name=*newt_core::book_index::FileName::new(digest,newt_core::book_index::FileKind::Index).as_bytes();
        assert!(newt_core::page_cache::CacheStorage::file_size(&mut pages,&name)<0);
        drop(pages);
        let mut receipt=sd_revision::SdRevisionStorage::lend(&mut session.receipt,&mut native,&route).unwrap();
        let mut revision=ImportedRevision::compact(&mut receipt,&mut session.whole,&mut session.block,&mut session.scratch);
        assert!(revision.begin_import(&mut **source));
        session.source=Some(ReaderSource::new(revision).into_state());
    }).unwrap();
    session.verification.control().flags.set_prepared(true);
    runtime.session=Some(session);
    let indexed=image::index_loans::index(runtime,home,logical,true);
    assert!(indexed.built() && indexed.source_reopened() && !indexed.verified_now(),"a 600-spine-item book must spill and reopen: {indexed:?}");
    assert_eq!(runtime.session.as_ref().unwrap().verification.state(),Verification::Pending);
    image::drive::PREPARED.with(|prepared| prepared.set(None));
    image::execute_open(runtime,home,ui,logical,events,indexed);
    assert_eq!(image::drive::PREPARED.with(Cell::get),Some(true),
        "a spill-reopened first open must finish the prepared import");
    // Route check only: pending verification without the prepared flag, as
    // after hint admission. The revision here is still provisional, because
    // the host RunLoan never completes an import; only the route is checked.
    let session=runtime.session.as_mut().unwrap();let raw=runtime.source.as_mut().unwrap();
    assert!(session.verification.finish_with_update(&mut **raw,digest,&mut[0;4096],&mut||true,&mut|hash,bytes|
        newt_phased_reader::imported_revision::RevisionHasher::update(hash,bytes)));
    let indexed=image::index_loans::index(runtime,home,logical,true);
    assert!(!indexed.built() && !indexed.source_reopened());
    let session=runtime.session.as_mut().unwrap();
    assert!(!session.verification.control().flags.prepared());
    assert!(session.verification.begin(&**runtime.source.as_ref().unwrap(),reader_hash::MbedHasher::default));
    image::drive::PREPARED.with(|prepared| prepared.set(None));
    image::execute_open(runtime,home,ui,logical,events,indexed);
    assert_eq!(image::drive::PREPARED.with(Cell::get),Some(false),"a pending open that is not prepared stays Selected");
    println!("PASS spill-reopened first open: 600 spine items open Prepared; pending open without prepare stays Selected");
}

impl nvs::Inventory for Flash {
    fn visit_entries(&mut self, _: &mut dyn FnMut(&[u8], &[u8], bool) -> bool)
        -> core::result::Result<(), nvs::NativeError> {
        panic!("unavailable NVS must not be scanned")
    }
}
