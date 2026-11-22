// SPDX-License-Identifier: MIT
//! One actual POSIX DIR owner, with names borrowed from its existing dirent.
use crate::{Error, Result, UiIo, status};
use core::{
    ffi::{CStr, c_char, c_void},
    marker::PhantomData,
    ptr::NonNull,
};

// SAFETY: one synchronous readdir on this live DIR, no retained Rust pointer.
// Its terminated name stays SDK-owned until this DIR's next read or close.
unsafe extern "C" {
    fn newt_io_readdir_entry(
        directory: *mut c_void,
        name: *mut *const c_char,
        kind: *mut u32,
    ) -> i32;
    fn newt_io_readonly_opendir_errno(path: *const c_char, directory: *mut *mut c_void) -> i32;
}

/// The native opendir allocation itself; no copied names, window or controller.
/// The UI loan and native pointer exclude Clone/Send/Sync. Explicit close ends
/// normal scopes; Drop also releases an owner whose caller unwinds early.
pub struct Listing<'ui> {
    pointer: Option<NonNull<c_void>>,
    _ui: PhantomData<&'ui UiIo>,
}
impl<'ui> Listing<'ui> {
    pub fn open(_io: &'ui UiIo, path: &CStr) -> Result<Self> {
        let mut pointer = core::ptr::null_mut();
        // SAFETY: terminated borrowed path and exclusive pointer output; the
        // actual native DIR is owned here until its one explicit/drop close.
        status(unsafe { crate::newt_io_opendir(path.as_ptr(), &mut pointer) })?;
        Ok(Self {
            pointer: Some(NonNull::new(pointer).ok_or(Error::Other)?),
            _ui: PhantomData,
        })
    }
    pub fn next(&mut self) -> Result<Option<&CStr>> {
        Ok(self.next_entry()?.map(|(name, _)| name))
    }
    pub(crate) fn open_errno(_io: &'ui UiIo, path: &CStr) -> core::result::Result<Self, i32> {
        let mut pointer = core::ptr::null_mut();
        // SAFETY: the UI loan owns this synchronous SDK call. A successful
        // opendir transfers its unique DIR into the same existing owner.
        let error = unsafe { newt_io_readonly_opendir_errno(path.as_ptr(), &mut pointer) };
        if error != 0 {
            return Err(error);
        }
        let pointer = NonNull::new(pointer).ok_or(-1)?;
        Ok(Self {
            pointer: Some(pointer),
            _ui: PhantomData,
        })
    }
    pub(crate) fn next_entry(&mut self) -> Result<Option<(&CStr, u32)>> {
        let pointer = self.pointer.ok_or(Error::Other)?;
        let mut name = core::ptr::null();
        let mut kind = 0;
        // SAFETY: exclusive live DIR; returned loan is tied to &mut self, so
        // another next/close/drop cannot invalidate it while Rust can use it.
        if unsafe { newt_io_readdir_entry(pointer.as_ptr(), &mut name, &mut kind) } != 0 {
            return Err(Error::Other);
        }
        if name.is_null() {
            return Ok(None);
        }
        // SAFETY: actual readdir supplies a complete terminated SDK-owned name,
        // valid until this same DIR's next mutable operation or destruction.
        Ok(Some((unsafe { CStr::from_ptr(name) }, kind)))
    }
    pub fn with_next<R>(&mut self, work: impl FnOnce(Option<&CStr>) -> R) -> Result<R> {
        Ok(work(self.next()?))
    }
    pub fn close(mut self) -> bool {
        self.close_inner()
    }
    fn close_inner(&mut self) -> bool {
        let Some(pointer) = self.pointer.take() else {
            return true;
        };
        // SAFETY: unique original live DIR, consumed before native close so
        // an uncertain failure never causes a duplicate attempt from Drop.
        status(unsafe { crate::newt_io_closedir(pointer.as_ptr()) }).is_ok()
    }
}
impl Drop for Listing<'_> {
    fn drop(&mut self) {
        self.close_inner();
    }
}
