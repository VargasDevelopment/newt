// SPDX-License-Identifier: MIT
//! Frozen Home cover session (home_cover.h), with no held source/Board loan.
use super::home_refresh::HomeRefresh;
use crate::library_covers::{CoverBookFacts, PreparedCovers};
use newt_core::sd_resume::SdResumeRecord;
use newt_native_io::UiIo;
use newt_sdk_heap::Fixed;
use newt_stage_home::{PreparedHomeCover, prepare_home_cover};

#[repr(C)]
struct Workspace {
    pixels: [u8; 3384],
    ready: bool,
    identity: SdResumeRecord,
}
const _: () = assert!(core::mem::size_of::<Workspace>() == 3528);

#[derive(Default)]
pub struct HomeCover {
    book: u64,
    workspace: Option<Fixed<Workspace>>,
    seen: u32,
    eligible: bool,
    attempted: bool,
    current: bool,
    facts: CoverBookFacts,
    step: Step,
}
/// How the session's one read ended, for the DEV `HOME_COVER` row.
#[derive(Clone, Copy, Default)]
#[repr(u8)]
pub enum Step {
    #[default] None = 0, Unmounted = 1, Loan = 2, Memory = 3, Identity = 4, NoCover = 5, NotReady = 6, Ready = 7,
}
impl HomeCover {
    /// A rejected native I/O loan is this session's one failed preparation;
    /// it does not assert anything about the cover file's presence or bytes.
    pub fn failed_attempt(&mut self, step: Step) {
        if self.eligible && !self.attempted { self.attempted = true; self.step = step; }
    }
    pub fn release(&mut self) {
        self.workspace = None;
        self.attempted = false;
        self.current = false;
        self.step = Step::None;
    }
    /// (eligible, attempted, step, ready): numbers only, never the book.
    pub fn report(&self) -> (bool, bool, Step, bool) {
        (self.eligible, self.attempted, self.step, self.held().is_some_and(|cover| cover.ready))
    }
    /// The caller computes eligibility with the real selection verification
    /// and Home card flags; generation is its actual cover publication word.
    pub fn observe(&mut self, eligible: bool, book: u64, generation: u32, policy: &mut HomeRefresh) {
        let book = if eligible { book } else { 0 };
        let changed = eligible != self.eligible || book != self.book || generation != self.seen;
        if changed { self.facts = CoverBookFacts::default(); }
        if changed || !self.current {
            self.release();
            self.eligible = eligible;
            self.book = book;
            self.seen = generation;
            self.current = true;
            policy.invalidate();
        }
    }
    /// One read attempt per Home session, with the selected owner's validated
    /// identity copied into the existing heap workspace by the required loan.
    pub fn read_once(&mut self, io: &UiIo, retain: &mut dyn FnMut(&mut SdResumeRecord) -> bool) {
        if self.attempted || !self.eligible { return; }
        self.attempted = true;
        let Some(mut workspace) = Fixed::try_new_with(|| Workspace {
            pixels: [0; 3384], ready: false, identity: SdResumeRecord::from_words(&[0; 35]),
        }) else { self.step = Step::Memory; return; };
        if !retain(&mut workspace.identity) || !workspace.identity.valid() { self.step = Step::Identity; return; }
        if self.facts.record != workspace.identity.checksum {
            self.facts = CoverBookFacts { record: workspace.identity.checksum, ..CoverBookFacts::default() };
        }
        if self.facts.no_cover { self.step = Step::NoCover; return; }
        let mut native = io.port();
        let mut covers = PreparedCovers::new(&mut native, Some(&mut self.facts));
        let Workspace { pixels, ready, identity } = &mut *workspace;
        *ready = prepare_home_cover(&mut covers, Some(&identity.filename), pixels).ready;
        self.step = if *ready { Step::Ready } else { Step::NotReady };
        if *ready { self.workspace = Some(workspace); }
    }
    pub fn held(&self) -> Option<PreparedHomeCover<'_>> {
        self.workspace.as_ref().map(|workspace| PreparedHomeCover {
            pixels: &workspace.pixels, ready: workspace.ready,
        })
    }
}
