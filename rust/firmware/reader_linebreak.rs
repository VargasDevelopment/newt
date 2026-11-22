// SPDX-License-Identifier: MIT
//! Allocation-free firmware call into the pinned libunibreak implementation.
use newt_core::chapter_layout::types::LineBreak;
unsafe extern "C" {
    fn set_linebreaks_utf8(
        text: *const u8,
        len: usize,
        lang: *const core::ffi::c_char,
        breaks: *mut core::ffi::c_char,
    );
}
pub struct NativeBreak;
impl LineBreak for NativeBreak {
    fn breaks(&mut self, t: &[u8], lang: Option<&core::ffi::CStr>, out: &mut [u8]) {
        assert_eq!(t.len(), out.len());
        unsafe {
            set_linebreaks_utf8(
                t.as_ptr(),
                t.len(),
                lang.map_or(core::ptr::null(), core::ffi::CStr::as_ptr),
                out.as_mut_ptr().cast(),
            )
        }
    }
}
