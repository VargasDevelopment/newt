// SPDX-License-Identifier: MIT
//! Exact cover-only native stat/heap wire; ordinary Native Stat is unchanged.
use core::ffi::{c_char, CStr};
use crate::{status, Result, UiIo};
#[path="cover_observer.rs"]
pub mod observer;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Facts {
    pub size: i64, pub inode: u64, pub device: u64, pub seconds: i64,
    pub links: u64, pub mode: u32,
}
impl Facts {
    pub fn regular(self) -> bool { self.mode & 0o170000 == 0o100000 }
    pub fn directory(self) -> bool { self.mode & 0o170000 == 0o040000 }
    pub fn same(self, other: Self) -> bool {
        self.regular() && other.regular() && self.size > 0 && self.size == other.size
            && self.seconds >= 0 && self.seconds == other.seconds
            && self.device == other.device && self.inode == other.inode
    }
}
const _: () = assert!(core::mem::size_of::<Facts>() == 48);
// SAFETY: linked cover.cpp accepts live terminated paths/exclusive 48B outputs,
// returns synchronously, retains no pointers and performs exactly one SDK call.
unsafe extern "C" {
    fn newt_cover_stat(path: *const c_char, out: *mut Facts) -> i32;
    fn newt_cover_fstat(fd: i32, out: *mut Facts) -> i32;
    fn newt_cover_complete_handle_metadata() -> bool;
    fn newt_cover_rename_replaces_destination() -> bool;
    fn newt_cover_heap_supported() -> bool;
    fn newt_cover_heap_free_bytes() -> usize;
    fn newt_cover_heap_largest() -> usize;
    fn newt_cover_heap_allocate(bytes: usize) -> *mut u8;
    fn newt_cover_heap_charged(pointer: *mut u8) -> usize;
    fn newt_cover_heap_release(pointer: *mut u8);
}
pub fn complete_handle_metadata()->bool{
    // SAFETY: native platform's actual fstat capability, scalar read-only query.
    unsafe{newt_cover_complete_handle_metadata()}
}
pub fn rename_replaces_destination()->bool{
    // SAFETY: native filesystem rename capability, scalar read-only query.
    unsafe{newt_cover_rename_replaces_destination()}
}
pub fn stat(_io: &UiIo, path: &CStr) -> Result<Facts> {
    let mut out = Facts::default();
    // SAFETY: terminated borrowed path and exclusive exact wire output.
    status(unsafe { newt_cover_stat(path.as_ptr(), &mut out) })?; Ok(out)
}
pub fn fstat(_io: &UiIo, fd: i32) -> Result<Facts> {
    let mut out = Facts::default();
    // SAFETY: caller's UI domain owns fd; output is exclusive and synchronous.
    status(unsafe { newt_cover_fstat(fd, &mut out) })?; Ok(out)
}
pub fn open(io:&UiIo,path:&CStr,write:bool)->Result<i32>{
    io.port().open_cstr(path,if write{newt_native_storage::Open::CreateExclusiveWriteOnly}
        else{newt_native_storage::Open::SourceReadOnly})
}
pub fn mkdir(_io:&UiIo,path:&CStr)->Result<()>{
    // SAFETY: synchronous existing native mkdir0700, terminated path lives here.
    status(unsafe{crate::newt_io_mkdir(path.as_ptr())})
}
pub fn unlink(_io:&UiIo,path:&CStr)->Result<()>{
    // SAFETY: exact terminated byte path; existing native performs one unlink.
    status(unsafe{crate::newt_io_unlink(path.as_ptr())})
}
pub fn rename(_io:&UiIo,from:&CStr,to:&CStr)->Result<()>{
    // SAFETY: both terminated paths live throughout one native rename call.
    status(unsafe{crate::newt_io_rename(from.as_ptr(),to.as_ptr())})
}
pub fn supported() -> bool {
    // SAFETY: read-only pinned profile query, no pointers or retained state.
    unsafe { newt_cover_heap_supported() }
}
pub fn free_bytes() -> usize {
    // SAFETY: initialized SDK/UI context is required by the cover caller.
    unsafe { newt_cover_heap_free_bytes() }
}
pub fn largest() -> usize {
    // SAFETY: same live SDK scalar query; unsupported profiles report zero.
    unsafe { newt_cover_heap_largest() }
}
/// # Safety
/// Call only in initialized UI-task SDK context; own a nonnull result until
/// exactly one release. The pinned allocator returns max_align_t alignment.
pub unsafe fn allocate(bytes: usize) -> *mut u8 {
    // SAFETY: caller supplies the SDK lifetime; null transfers no ownership.
    unsafe { newt_cover_heap_allocate(bytes) }
}
/// # Safety
/// pointer must be a live, uniquely owned result of allocate.
pub unsafe fn charged(pointer: *mut u8) -> usize {
    // SAFETY: caller proves the original live SDK allocation.
    unsafe { newt_cover_heap_charged(pointer) }
}
/// # Safety
/// pointer is null or the original live allocate result, never an interior view.
pub unsafe fn release(pointer: *mut u8) {
    // SAFETY: caller transfers its unique original allocation exactly once.
    unsafe { newt_cover_heap_release(pointer) }
}
