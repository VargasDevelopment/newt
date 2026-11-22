// SPDX-License-Identifier: MIT

//! Platform-only owner and leaf exports. C++ serializes every leaf access.
//! The owner has a constant initializer and needs no Arduino setup callback.
//! No reference made here escapes the call, including init begin/end.
use crate::guard::{Guard, Partition, Refusals};
use crate::save_notice::NvsIssue;
use core::ffi::c_void;

// No Rust reference is formed here. The C++ leaf boundary owns serialization.
static mut OWNER: Guard = Guard::new();

#[unsafe(no_mangle)]
pub extern "C" fn newt_guard_owner() -> *mut c_void {
    (&raw mut OWNER).cast::<c_void>()
}

fn partition(present: u8, kind: u8, subtype: u8, size: usize) -> Option<Partition> {
    (present != 0).then_some(Partition {
        kind,
        subtype,
        size,
    })
}

// SAFETY CONTRACT for all exports: state points to the unique live Guard at a
// stable address; no concurrent/reference access overlaps this call. C++ must
// release every loan before invoking SDK code, which may reenter these exports.
// sdk_glue.cpp guardCall holds the fixed single-core port critical section for
// exactly one leaf, including queries. No leaf allocates, blocks or calls SDK.
// No reference is kept across the native SDK operation between begin/finish.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_begin_init(state: *mut c_void) -> u8 {
    u8::from(unsafe { &mut *state.cast::<Guard>() }.begin_initialization())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_finish_init(state: *mut c_void, result: i32) -> i32 {
    unsafe { &mut *state.cast::<Guard>() }.finish_initialization(result)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_deny_write(
    state: *mut c_void,
    present: u8,
    kind: u8,
    subtype: u8,
    size: usize,
) -> u8 {
    u8::from(
        unsafe { &mut *state.cast::<Guard>() }.deny_write(partition(present, kind, subtype, size)),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_deny_erase(
    state: *mut c_void,
    present: u8,
    kind: u8,
    subtype: u8,
    extent: usize,
    offset: usize,
    size: usize,
) -> u8 {
    u8::from(unsafe { &mut *state.cast::<Guard>() }.deny_erase(
        partition(present, kind, subtype, extent),
        offset,
        size,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_deny_open(state: *mut c_void, mode: i32) -> u8 {
    u8::from(unsafe { &mut *state.cast::<Guard>() }.deny_open(mode))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_deny_format(state: *mut c_void) -> i32 {
    unsafe { &mut *state.cast::<Guard>() }.deny_format()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_ready(state: *const c_void) -> u8 {
    u8::from(unsafe { &*state.cast::<Guard>() }.storage_ready())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_issue(state: *const c_void) -> u8 {
    match unsafe { &*state.cast::<Guard>() }.storage_issue() {
        NvsIssue::None => 0,
        NvsIssue::Version => 1,
        NvsIssue::Recovery => 2,
        NvsIssue::Unavailable => 3,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_fence(state: *mut c_void, raised: u8) {
    unsafe { &mut *state.cast::<Guard>() }.fence_for_wifi(raised != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_fenced(state: *const c_void) -> u8 {
    u8::from(unsafe { &*state.cast::<Guard>() }.flash_fenced())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_refusals(state: *const c_void) -> Refusals {
    unsafe { &*state.cast::<Guard>() }.refusals()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_guard_problem(state: *const c_void) -> *const core::ffi::c_char {
    unsafe { &*state.cast::<Guard>() }
        .storage_issue()
        .text()
        .map_or(core::ptr::null(), |text| text.as_ptr())
}
