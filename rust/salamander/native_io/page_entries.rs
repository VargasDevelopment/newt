//! Synchronous borrow of Newt's unchanged native page-entry walker. MIT.
use super::{OsNative, c_void};
use core::ffi::CStr;
use newt_native_storage::page_entries::{PageEntries, PageEntry};

// SAFETY: this function pointer is invoked only under the synchronous context
// and name-lifetime contract of receive; no callback may unwind into C++.
type Visitor = unsafe extern "C" fn(*mut c_void, *const u8, usize, u64, i64, u32) -> u32;
// SAFETY: the matching C entry calls only during the walk and retains no
// callback, context or name; fixed-width scalars never expose C++ layout.
unsafe extern "C" {
    fn newt_io_walk_pages(folder: *const core::ffi::c_char, sized: u32,
        visit: Visitor, context: *mut c_void) -> i32;
}
struct Visit<'call, 'ui> {
    native: &'call mut OsNative<'ui>,
    visitor: &'call mut dyn FnMut(&mut OsNative<'ui>, PageEntry<'_>) -> bool,
    good: bool,
}
// SAFETY: only newt_io_walk_pages invokes this with the live exclusive Visit.
// A non-null name covers exactly length immutable bytes until this call ends;
// it is disjoint from the Rust context/native owner. The C++ directory read
// has returned before the short native reborrow, and never retains its result.
unsafe extern "C" fn receive(context: *mut c_void, name: *const u8, length: usize,
    bytes: u64, time: i64, regular: u32,
) -> u32 {
    if context.is_null() { return 0; }
    // SAFETY: the synchronous caller owns this Visit, uniquely for this call.
    let context = unsafe { &mut *context.cast::<Visit<'_, '_>>() };
    if regular > 1 || length > isize::MAX as usize
        || (name.is_null() && length != 0) || (!name.is_null() && length == 0)
    { context.good = false; return 0; }
    let name = if name.is_null() { None } else {
        // SAFETY: matching C++ walker provides a valid, immutable name of this
        // length; it cannot alias the exclusive Rust context or be retained.
        Some(unsafe { core::slice::from_raw_parts(name, length) })
    };
    context.good = context.good && (context.visitor)(context.native,
        PageEntry { name, bytes, time, regular: regular == 1 });
    u32::from(context.good)
}
impl PageEntries for OsNative<'_> {
    fn walk_entries(&mut self, folder: &CStr, sized: bool,
        visitor: &mut dyn FnMut(&mut Self, PageEntry<'_>) -> bool,
    ) -> bool {
        let mut context = Visit { native: self, visitor, good: true };
        // SAFETY: folder is terminated in the caller's existing backing.
        // Context and callback remain live/exclusive until the unchanged native
        // walker returns. No parser/frame loan or pointer escapes this call.
        let result = unsafe { newt_io_walk_pages(folder.as_ptr(), u32::from(sized),
            receive, (&mut context as *mut Visit<'_, '_>).cast()) };
        result == 1 && context.good
    }
}
