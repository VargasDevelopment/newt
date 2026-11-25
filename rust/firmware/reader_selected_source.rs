// SPDX-License-Identifier: MIT
//! Selected source state at frozen SdLibrary::file_/source_ scope. This
//! REPLACES their storage; it is not a spare source beside another file owner.
//! Parent selection closes its optional revision session BEFORE close/open.
use newt_core::zip_window::source::BookSource;
use newt_native_storage::{Authority, Native};
use newt_native_storage::sd_source::SdSourceState;
use newt_native_storage::source_file::{File, FileBookSource, SourceState};

/// No native backend or policy pointer survives a short operation loan.
/// Root's app owner explicitly closes before leaving the mounted UI domain.
#[must_use = "close the selected source before its native domain ends"]
pub(crate) struct SelectedSource {
    file: SdSourceState,
    bound: Option<SourceState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceLoanError { Absent, BindingLost }

pub(crate) trait FileObserver {
    fn micros(&self) -> u32;
    fn io(&self, operation: newt_core::reader_work_profile::ReaderIoOperation, elapsed: u32, bytes: u32);
}

/// Only a lexical observation adapter over the SAME original file. Every
/// operation delegates once; no Native I/O, state or descriptor is duplicated.
struct ObservedFile<'a, F> { file: &'a mut F, observer: Option<&'a dyn FileObserver> }
impl<F: File> ObservedFile<'_, F> {
    fn observe(&mut self, operation: newt_core::reader_work_profile::ReaderIoOperation,
        work: impl FnOnce(&mut F) -> (u64, u32),
    ) -> u64 {
        let began = self.observer.map(|observer| observer.micros());
        let (result, bytes) = work(self.file);
        if let (Some(observer), Some(began)) = (self.observer, began) {
            observer.io(operation, observer.micros().wrapping_sub(began), bytes);
        }
        result
    }
}
impl<F: File> File for ObservedFile<'_, F> {
    fn opened(&mut self) -> bool { self.file.opened() }
    fn is_directory(&mut self) -> bool { self.file.is_directory() }
    fn size(&mut self) -> u64 {
        self.observe(newt_core::reader_work_profile::ReaderIoOperation::Size, |file| (file.size(), 0))
    }
    fn position(&mut self) -> u64 {
        self.observe(newt_core::reader_work_profile::ReaderIoOperation::Position, |file| (file.position(), 0))
    }
    fn seek(&mut self, at: u32) -> bool {
        self.observe(newt_core::reader_work_profile::ReaderIoOperation::Seek, |file| (u64::from(file.seek(at)), 0)) != 0
    }
    fn read(&mut self, output: &mut [u8]) -> usize {
        self.observe(newt_core::reader_work_profile::ReaderIoOperation::Read, |file| {
            let bytes = file.read(output);
            (bytes as u64, bytes as u32)
        }) as usize
    }
}

impl SelectedSource {
    pub(crate) fn new() -> Self {
        Self { file: SdSourceState::new(), bound: None }
    }

    pub(crate) fn bound(&self) -> bool { self.bound.is_some() }

    /// The caller supplies the actual mounted/path admission; a failed path
    /// admission closes at SdLibrary::openFilename's existing outer boundary.
    /// After this call, no reader/Home transition opens the file implicitly.
    pub(crate) fn open<N: Native>(&mut self, native: &mut N, path: &str,
        cancel: Option<&dyn Authority>) -> bool {
        self.open_with_observer(native, path, cancel, None)
    }
    pub(crate) fn open_observed<N: Native>(&mut self, native: &mut N, path: &str,
        cancel: Option<&dyn Authority>, observer: Option<&dyn FileObserver>) -> bool {
        match observer {
            Some(observer) => self.open_with_observer(native, path, cancel, Some(observer)),
            None => self.open(native, path, cancel),
        }
    }
    fn open_with_observer<N: Native>(&mut self, native: &mut N, path: &str,
        cancel: Option<&dyn Authority>, observer: Option<&dyn FileObserver>) -> bool {
        self.close(native);
        let mut file = self.file.lend(native);
        if !file.open(path) { return false; }
        let mut observed = ObservedFile { file: &mut file, observer };
        let mut source = FileBookSource::new(cancel);
        if !source.bind(&mut observed) {
            source.release();
            file.close();
            return false;
        }
        match source.into_state() {
            Ok((bound, _)) => { self.bound = Some(bound); true }
            Err(source) => {
                source.release();
                file.close();
                false
            }
        }
    }

    /// Each operation uses the SAME fd/cursor and cached bound size. No bind,
    /// fstat, source probe, cursor reset or path lookup is added by lending.
    /// Only BookSource's existing operations can run inside the callback.
    /// The resulting binding is returned also on a caught host unwind.
    pub(crate) fn with_source<N: Native, R>(&mut self, native: &mut N,
        cancel: Option<&dyn Authority>,
        operation: impl for<'op> FnOnce(&'op mut dyn BookSource) -> R,
    ) -> Result<R, SourceLoanError> {
        self.with_source_observed(native, cancel, None, operation)
    }
    pub(crate) fn with_source_observed<N: Native, R>(&mut self, native: &mut N,
        cancel: Option<&dyn Authority>, observer: Option<&dyn FileObserver>,
        operation: impl for<'op> FnOnce(&'op mut dyn BookSource) -> R,
    ) -> Result<R, SourceLoanError> {
        let Self { file, bound } = self;
        let state = bound.take().ok_or(SourceLoanError::Absent)?;
        let mut file = file.lend(native);
        let mut observed = ObservedFile { file: &mut file, observer };
        let mut loan = SourceLoan { source: state.lend(&mut observed, cancel), saved: &mut *bound };
        let result = operation(&mut loan.source);
        drop(loan);
        if bound.is_some() { Ok(result) } else { Err(SourceLoanError::BindingLost) }
    }

    /// Mirrors detach then the actual native close. The uncertain-close path
    /// still invalidates the descriptor/cursor through the existing body.
    pub(crate) fn close<N: Native>(&mut self, native: &mut N) {
        self.bound = None;
        self.file.close(native);
    }
}

// Only the short view is replaced during Drop. There is no copied descriptor,
// state snapshot, fake native port or cleanup I/O at the end of a reader loan.
struct SourceLoan<'view, 'saved, F: File> {
    source: FileBookSource<'view, F>,
    saved: &'saved mut Option<SourceState>,
}
impl<F: File> Drop for SourceLoan<'_, '_, F> {
    fn drop(&mut self) {
        let source = core::mem::replace(&mut self.source, FileBookSource::new(None));
        match source.into_state() {
            Ok((state, _)) => *self.saved = Some(state),
            Err(source) => source.release(),
        }
    }
}

#[path = "reader_selected_index_loan.rs"]
mod index_loan;
