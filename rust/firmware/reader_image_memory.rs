// SPDX-License-Identifier: MIT
//! One reader backing at a time, as frozen SampleReader::close/open require.
use newt_c3_board::reader_memory::ReaderMemory;
#[cfg(feature = "embedded-sample")]
use newt_c3_board::sample_memory::SampleMemory;
use newt_core::tracker::ReadingTracker;
use newt_phased_reader::PageInfo;
#[cfg(feature = "embedded-sample")]
use newt_reader_ui::ReaderOwnerState;

pub(super) enum ReaderBacking {
    Closed,
    /// A Back or Power press stopped its open: a pause, not a result. Its
    /// session is released and a reopen is owed, so it paints nothing until
    /// that press leads away or idle reopens it.
    Interrupted,
    External(ReaderMemory),
    #[cfg(feature = "embedded-sample")]
    Sample(SampleMemory),
}
// Frozen SampleReader held two optional owner pointers. The variant and its
// one live pointer fit that same bound; no second backing can be represented.
const _: () = assert!(core::mem::size_of::<ReaderBacking>() <= 2 * core::mem::size_of::<usize>());

pub(super) struct ReadingView<'a> {
    pub(super) has_progress: bool,
    pub(super) opened: bool,
    pub(super) page: PageInfo,
    pub(super) tracker: &'a ReadingTracker,
}

impl ReaderBacking {
    pub(super) fn external(&self) -> Option<&ReaderMemory> {
        match self { Self::External(memory) => Some(memory), _ => None }
    }
    pub(super) fn external_mut(&mut self) -> Option<&mut ReaderMemory> {
        match self { Self::External(memory) => Some(memory), _ => None }
    }
    #[cfg(feature = "embedded-sample")]
    pub(super) fn sample(&self) -> Option<&SampleMemory> {
        match self { Self::Sample(memory) => Some(memory), _ => None }
    }
    #[cfg(feature = "embedded-sample")]
    pub(super) fn sample_mut(&mut self) -> Option<&mut SampleMemory> {
        match self { Self::Sample(memory) => Some(memory), _ => None }
    }
    pub(super) fn has_sample(&self) -> bool {
        match self {
            #[cfg(feature = "embedded-sample")]
            Self::Sample(_) => true,
            _ => false,
        }
    }
    pub(super) fn is_closed(&self) -> bool { matches!(self, Self::Closed | Self::Interrupted) }
    pub(super) fn reading(&self, _sample_status: newt_core::zip_container::Status) -> Option<ReadingView<'_>> {
        match self {
            Self::Closed | Self::Interrupted => None,
            Self::External(memory) => {
                let view = memory.view();
                Some(ReadingView { has_progress: view.owner.state.has_progress(), opened: view.owner.state.opened(),
                    page: view.owner.state.position_info(view.reader.info(), view.reader.selection().spines).page,
                    tracker: &view.owner.tracker })
            }
            #[cfg(feature = "embedded-sample")]
            Self::Sample(memory) => {
                let view = memory.view();
                let owner = ReaderOwnerState::from_open(true, _sample_status, view.spines);
                Some(ReadingView { has_progress: true, opened: owner.opened(),
                    page: owner.position_info(view.reader.info(), view.spines).page,
                    tracker: view.tracker })
            }
        }
    }
}
