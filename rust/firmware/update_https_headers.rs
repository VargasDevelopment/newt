// SPDX-License-Identifier: MIT
//! One synchronous receive-headers loan; the client retains no Rust reference.
use core::ffi::{c_char, c_void, CStr};
use newt_core::{network_clock::parse_http_date, update_online::{HttpHead, URL_BYTES}};
use super::native;

struct Headers<'a> {
    head: &'a mut HttpHead,
    location: &'a mut [u8; URL_BYTES],
    bytes: usize,
    cut: bool,
}
struct Attached { client: *mut c_void }
impl Drop for Attached {
    fn drop(&mut self) {
        // SAFETY: the client is still live; clear user_data before the stack
        // context or its head/location loans end, including unwinding.
        if !unsafe { native::nh_http_set_context(self.client, core::ptr::null_mut()) } {
            crate::app_power::stop();
        }
    }
}

pub(super) fn fetch(client: *mut c_void, head: &mut HttpHead, location: &mut [u8; URL_BYTES]) -> Option<(i64, bool)> {
    let mut headers = Headers { head, location, bytes: 0, cut: false };
    // SAFETY: synchronous, non-async SDK receive-headers owns this context only
    // until Attached clears it; native forwards whole NUL-terminated headers.
    if !unsafe { native::nh_http_set_context(client, (&raw mut headers).cast()) } { return None; }
    let attached = Attached { client };
    // SAFETY: the unique client is open and its only live callback loans are
    // the stack Headers fields above. SDK completes header processing here.
    let length = unsafe { native::nh_http_headers(client) };
    drop(attached);
    Some((length, headers.cut))
}

// SAFETY: this exact symbol is called only by the synchronous native header
// trampoline, with its currently attached live Headers and whole C strings.
#[unsafe(no_mangle)]
unsafe extern "C" fn newt_https_note_header(context: *mut c_void, key: *const c_char, value: *const c_char) {
    // SAFETY: native checks all three pointers and calls serially inside fetch;
    // no Rust access to the context overlaps this callback's exclusive loan.
    let (headers, key, value) = unsafe {
        (&mut *context.cast::<Headers<'_>>(), CStr::from_ptr(key), CStr::from_ptr(value))
    };
    let text = value.to_bytes();
    headers.bytes = headers.bytes.wrapping_add(key.to_bytes().len()).wrapping_add(text.len());
    if headers.bytes > 16 * 1024 { headers.cut = true; }
    if key.to_bytes().eq_ignore_ascii_case(b"Date") { headers.head.date = parse_http_date(Some(value)); }
    if !key.to_bytes().eq_ignore_ascii_case(b"Location") { return; }
    if text.len() < headers.location.len() && headers.location[0] == 0 {
        headers.location[..text.len() + 1].copy_from_slice(value.to_bytes_with_nul());
    } else { headers.head.location_cut = true; }
}
