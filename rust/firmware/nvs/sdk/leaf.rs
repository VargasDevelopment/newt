// SPDX-License-Identifier: MIT

//! One implementation of the stateless, guarded IDF 5.5.2 NVS calls.
use super::Sdk552;
use crate::nvs::NativeError;
use core::ffi::{CStr,c_char,c_void};

// SAFETY: the unique Sdk552 constructor requires the actual pinned shim and
// Newt guard. Every call is synchronous, retains no Rust pointer and accesses
// only the supplied scalar/byte extent; SDK owns all mutable native state.
unsafe extern "C" {
    fn newt_nvs_ready() -> u8;
    fn newt_nvs_issue() -> u8;
    fn newt_nvs_open(name: *const c_char, readonly: u8, handle: *mut u32) -> i32;
    fn newt_nvs_close(handle: u32);
    fn newt_nvs_get(handle: u32, key: *const c_char, output: *mut c_void, size: *mut usize) -> i32;
    fn newt_nvs_set(handle: u32, key: *const c_char, bytes: *const c_void, size: usize) -> i32;
    fn newt_nvs_commit(handle: u32) -> i32;
    fn newt_nvs_erase(handle: u32, key: *const c_char) -> i32;
    fn newt_nvs_visit(visit: extern "C" fn(*mut c_void, *const c_char, *const c_char, u8) -> u8,
        context: *mut c_void) -> i32;
}

pub(super) fn visit_entries(_sdk: &Sdk552,
    visitor: &mut dyn FnMut(&[u8], &[u8], bool) -> bool) -> Result<(), NativeError> {
    extern "C" fn visit(context: *mut c_void, name: *const c_char, key: *const c_char, blob: u8) -> u8 {
        // SAFETY: the synchronous shim checks both fixed arrays are terminated;
        // context points to the exclusive callback reference below, never retained.
        unsafe {
            let visitor = &mut *context.cast::<&mut dyn FnMut(&[u8], &[u8], bool) -> bool>();
            u8::from(visitor(CStr::from_ptr(name).to_bytes(), CStr::from_ptr(key).to_bytes(), blob != 0))
        }
    }
    let mut callback = visitor;
    // SAFETY: shim visits only during this call, retains nothing, and releases
    // its iterator even when the callback refuses. No callback reenters NVS.
    NativeError::from_code(unsafe { newt_nvs_visit(visit, (&mut callback as *mut &mut dyn FnMut(&[u8], &[u8], bool) -> bool).cast()) })
}
pub(super) fn storage_ready(_sdk:&Sdk552)->bool {
    // SAFETY: the borrowed lease requires the linked Newt guard and matching ABI.
    unsafe { newt_nvs_ready() != 0 }
}
pub(super) fn storage_issue(_sdk:&Sdk552)->u8 {
    // SAFETY: as storage_ready; the guard's frozen NvsIssue value, unnamed values kept.
    unsafe { newt_nvs_issue() }
}
pub(super) fn open(_sdk:&Sdk552,name:&CStr,readonly:bool,handle:&mut u32)->Result<(),NativeError> {
    // SAFETY: CStr and exclusive handle live through this synchronous call.
    NativeError::from_code(unsafe { newt_nvs_open(name.as_ptr(),u8::from(readonly),handle) })
}
pub(super) fn close(_sdk:&Sdk552,handle:u32) {
    // SAFETY: backend passes only its successfully opened native handle.
    unsafe { newt_nvs_close(handle) }
}
pub(super) fn get_blob(_sdk:&Sdk552,handle:u32,key:&CStr,output:Option<&mut[u8]>,size:&mut usize)->Result<(),NativeError> {
    // Native is public; reject inconsistent capacity before FFI, unchanged.
    if output.as_ref().is_some_and(|buffer| *size>buffer.len()) {
        return NativeError::from_code(0x110c);
    }
    let pointer=output.map_or(core::ptr::null_mut(),|buffer|buffer.as_mut_ptr().cast());
    // SAFETY: pointers are live; non-null output has at least *size bytes.
    NativeError::from_code(unsafe { newt_nvs_get(handle,key.as_ptr(),pointer,size) })
}
pub(super) fn set_blob(_sdk:&Sdk552,handle:u32,key:&CStr,bytes:&[u8])->Result<(),NativeError> {
    // SAFETY: input pointers remain borrowed throughout the synchronous call.
    NativeError::from_code(unsafe { newt_nvs_set(handle,key.as_ptr(),bytes.as_ptr().cast(),bytes.len()) })
}
pub(super) fn commit(_sdk:&Sdk552,handle:u32)->Result<(),NativeError> {
    // SAFETY: shim retains no references and mutates no Rust lease state.
    NativeError::from_code(unsafe { newt_nvs_commit(handle) })
}
pub(super) fn erase_key(_sdk:&Sdk552,handle:u32,key:&CStr)->Result<(),NativeError> {
    // SAFETY: key is NUL-terminated and lives through the native call.
    NativeError::from_code(unsafe { newt_nvs_erase(handle,key.as_ptr()) })
}
pub(super) fn readback_durable(_sdk:&Sdk552)->bool {
    // Frozen nvs_blob_backend.h: pinned IDF writes flash before set returns;
    // commit is a no-op. This existing fact requires the constructor's contract.
    true
}
