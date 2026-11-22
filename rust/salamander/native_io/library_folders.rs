// SPDX-License-Identifier: MIT
//! Short loan of the frozen ESP32 FAT directory owner; Rust owns traversal.
use super::{Error, Result, UiIo};
use core::{ffi::{CStr, c_char, c_void}, marker::PhantomData, ptr::NonNull};
use newt_library_current::{DEPTH, Name, scan::{EntryRead, Folders, Read}};

#[repr(C)]
struct Entry {
    name: *const u8,
    length: usize,
    bytes: u32,
    folder: u32,
    hidden: u32,
}
// SAFETY: native FAT state and Rust Call must be live/disjoint for this one
// synchronous callback; it must not retain either pointer or unwind across C++.
type Scope = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
// SAFETY: link matching library_folders.cpp; lend valid paths/Entry outputs
// only synchronously. State ends with the scope; names expire on the next read.
unsafe extern "C" {
    fn newt_io_library_scope(levels: u32, callback: Scope, context: *mut c_void) -> u32;
    fn newt_io_library_open(state: *mut c_void, level: u32, path: *const c_char) -> u32;
    fn newt_io_library_next(state: *mut c_void, level: u32, entry: *mut Entry) -> u32;
    fn newt_io_library_close(state: *mut c_void, level: u32) -> u32;
}

/// The C++ callback owns exactly FF_DIR[DEPTH+1] and FILINFO. Neither their
/// pointers nor a filename loan can escape; close ordering belongs to scan.
pub struct LibraryFolders<'scope> {
    state: NonNull<c_void>,
    _owner: PhantomData<&'scope UiIo>,
}
struct Call<F, R> {
    work: Option<F>,
    result: Option<R>,
}

// SAFETY: matching native code invokes this once, synchronously, with its
// live exclusive directory state and the caller's disjoint Call. The higher
// ranked callback cannot retain the directory loan. No unwind may cross C++.
unsafe extern "C" fn receive<F, R>(state: *mut c_void, context: *mut c_void) -> u32
where F: for<'scope> FnOnce(&mut LibraryFolders<'scope>) -> R {
    let Some(state) = NonNull::new(state) else { return 0; };
    if context.is_null() { return 0; }
    // SAFETY: with_library_folders owns this Call until the native scope ends.
    let context = unsafe { &mut *context.cast::<Call<F, R>>() };
    let Some(work) = context.work.take() else { return 0; };
    context.result = Some(work(&mut LibraryFolders { state, _owner: PhantomData }));
    1
}

impl UiIo {
    /// Lend the actual transient FAT owner to one complete synchronous scan.
    /// The Rust traversal closes each successfully opened level. There is no
    /// native traversal, heap allocation or implicit extra close on scope exit.
    pub fn with_library_folders<R, F>(&self, work: F) -> Result<R>
    where F: for<'scope> FnOnce(&mut LibraryFolders<'scope>) -> R {
        let mut context = Call { work: Some(work), result: None };
        // SAFETY: UiIo proves the mounted, exclusive UI task. The native scope
        // checks the shared depth constant and lends only its own stack state;
        // context and callback remain live, and no parse/frame loan escapes.
        let complete = unsafe { newt_io_library_scope((DEPTH + 1) as u32,
            receive::<F, R>, (&mut context as *mut Call<F, R>).cast()) };
        if complete != 1 { return Err(Error::Other); }
        context.result.ok_or(Error::Other)
    }
}

impl Folders for LibraryFolders<'_> {
    fn open(&mut self, level: usize, path: &[u8]) -> bool {
        if level > DEPTH { return false; }
        // scan lends its existing 256-byte terminated path; do not allocate or
        // add a second path buffer merely to call the frozen mount conversion.
        let Ok(path) = CStr::from_bytes_until_nul(path) else { return false; };
        if path.to_bytes().len() >= 256 { return false; }
        // SAFETY: state belongs to this scope; path remains terminated/live.
        unsafe { newt_io_library_open(self.state.as_ptr(), level as u32, path.as_ptr()) == 1 }
    }
    fn next(&mut self, level: usize, _: &[u8]) -> EntryRead {
        if level > DEPTH { return EntryRead::Short(Read::Failed); }
        let mut entry = Entry { name: core::ptr::null(), length: 0, bytes: 0, folder: 0, hidden: 0 };
        // SAFETY: one f_readdir fills the native owner's FILINFO. Its filename
        // remains valid until another next call, which this exclusive loan
        // prevents while the slice below is inspected/copied.
        let read = unsafe { newt_io_library_next(self.state.as_ptr(), level as u32, &mut entry) };
        if read == 1 { return EntryRead::Short(Read::End); }
        if read != 2 || entry.name.is_null() || entry.length == 0
            || entry.length > isize::MAX as usize || entry.folder > 1 || entry.hidden > 1
        { return EntryRead::Short(Read::Failed); }
        // SAFETY: the matching bridge supplies the complete terminated FAT
        // name's exact byte extent, borrowed only for this synchronous method.
        let name = unsafe { core::slice::from_raw_parts(entry.name, entry.length) };
        let folder = entry.folder == 1;
        let hidden = entry.hidden == 1;
        let mut short: Name = [0; 96];
        if name.len() < short.len() {
            short[..name.len()].copy_from_slice(name);
            EntryRead::Short(Read::Entry { name: short, bytes: entry.bytes, folder, hidden })
        } else {
            EntryRead::LongEntry { folder, hidden, dot: name[0] == b'.',
                epub: name.len() > 5 && name[name.len() - 5..].eq_ignore_ascii_case(b".epub") }
        }
    }
    fn close(&mut self, level: usize) -> bool {
        if level > DEPTH { return false; }
        // SAFETY: Rust scan closes each opened level once before this scope
        // ends; return the actual f_closedir result without retry or masking.
        unsafe { newt_io_library_close(self.state.as_ptr(), level as u32) == 1 }
    }
}
