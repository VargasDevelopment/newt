// SPDX-License-Identifier: MIT
//! Index-only detach/reopen over the selected file's original physical owner.
use super::*;
use newt_phased_reader::{index_build::ReleasableSource, phase_storage::scratch_value};

trait ReopenFile: File {
    fn release_checked(&mut self) -> bool;
    fn reopen(&mut self, filename: &[u8]) -> bool;
}
impl<N: Native> ReopenFile for ObservedFile<'_, newt_native_storage::sd_source::SdSourceFile<'_, N>> {
    fn release_checked(&mut self) -> bool { self.file.release_checked() }
    // Path construction is gone before the builder runs and occurs again only
    // after publish has returned. No path buffer lives in an index ancestor.
    #[inline(never)]
    fn reopen(&mut self, filename: &[u8]) -> bool {
        let mut path = [0; b"/newt-sd/Newt/Books".len() + 2 + newt_core::library_name::FILENAME_BYTES];
        if !newt_core::library_catalog::library_book_path(Some(b"/newt-sd/Newt/Books\0"),Some(filename),Some(&mut path)) {
            return false;
        }
        let Some(path) = core::ffi::CStr::from_bytes_until_nul(&path).ok().and_then(|p| p.to_str().ok()) else { return false; };
        self.file.open(path)
    }
}
enum Binding<'a, F: ReopenFile> {
    Open(FileBookSource<'a, F>),
    Released(SourceState, &'a mut F),
}
struct IndexLoan<'a, 'saved, F: ReopenFile> {
    binding: Option<Binding<'a,F>>,
    saved: &'saved mut Option<SourceState>,
    filename: &'a [u8],
    cancel: Option<&'a dyn Authority>,
    bytes: u64,
}
impl<F: ReopenFile> BookSource for IndexLoan<'_, '_, F> {
    fn size(&self) -> u64 { self.bytes }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        match self.binding.as_mut() { Some(Binding::Open(source)) => source.read_at(at,out), _ => -1 }
    }
}
impl<F: ReopenFile> ReleasableSource for IndexLoan<'_, '_, F> {
    #[inline(never)]
    fn release(&mut self) -> bool {
        let Some(binding) = self.binding.take() else { return false; };
        let Binding::Open(source) = binding else { self.binding=Some(binding);return false; };
        match source.into_state() {
            Ok((state,file)) => {
                let closed=file.release_checked();
                self.binding=Some(Binding::Released(state,file));
                closed
            }
            Err(source) => { self.binding=Some(Binding::Open(source));false }
        }
    }
    #[inline(never)]
    fn reopen(&mut self) -> bool {
        let Some(binding) = self.binding.take() else { return false; };
        let Binding::Released(state,file) = binding else { self.binding=Some(binding);return false; };
        if self.cancelled() || !file.reopen(self.filename) {
            file.release_checked();
            return false;
        }
        // Reuse the existing bound byte-count comparison, with the same
        // cancellation and fstat checks as any FileBookSource empty probe.
        let mut source=state.lend(file,self.cancel);
        if source.read_at(0,&mut []) != 0 {
            if let Ok((_,file))=source.into_state() { file.release_checked(); }
            return false;
        }
        self.binding=Some(Binding::Open(source));
        true
    }
    fn cancelled(&self) -> bool { self.cancel.is_some_and(Authority::cancelled) }
}
impl<F: ReopenFile> Drop for IndexLoan<'_, '_, F> {
    #[inline(never)]
    fn drop(&mut self) {
        if let Some(Binding::Open(source))=self.binding.take() {
            match source.into_state() {
                Ok((state,_)) => *self.saved=Some(state),
                Err(source) => source.release(),
            }
        }
    }
}
impl SelectedSource {
    #[inline(always)]
    pub(crate) fn with_index_source<N: Native,R>(&mut self,native:&mut N,filename:&[u8],scratch:&mut[u8],
        cancel:Option<&dyn Authority>,observer:Option<&dyn FileObserver>,
        operation:impl FnOnce(&mut dyn ReleasableSource)->R,
    )->Result<R,SourceLoanError> {
        let Self {file,bound}=self;
        let state=bound.take().ok_or(SourceLoanError::Absent)?;
        let mut file=file.lend(native);
        let mut observed=ObservedFile {file:&mut file,observer};
        // The lifecycle owner uses only the existing logical-frame loan;
        // the builder receives separate shared and SDK-primary parse banks.
        let mut loan=scratch_value(scratch,|| {
            let source=state.lend(&mut observed,cancel);
            let bytes=source.size();
            Ok(IndexLoan {binding:Some(Binding::Open(source)),saved:&mut *bound,filename,cancel,bytes})
        }).map_err(|_|SourceLoanError::BindingLost)?;
        let result=operation(&mut *loan);
        drop(loan);
        if bound.is_some() {Ok(result)} else {Err(SourceLoanError::BindingLost)}
    }
}
