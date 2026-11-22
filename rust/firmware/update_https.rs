// SPDX-License-Identifier: MIT
//! Frozen HTTPS client and update-mode logical-frame TLS loan, one UI scope.
use core::{ffi::{c_void, CStr}, marker::PhantomData, ptr::{self, NonNull}};
use newt_core::{frame::Frame, network_clock::NetworkTime, update_online::{Clock, Http, HttpHead, URL_BYTES}};
#[path = "update_https_native.rs"]
mod native;
#[path = "update_https_headers.rs"]
mod headers;
#[path = "update_tls_frame.rs"]
mod tls;

/// The existing exclusive frame loan; no client, timer or heap until Http use.
/// Keep this scope outside Wi-Fi join/cleanup, so a kept frame takes the real
/// nonreturning update restart path before this owner's final safety boundary.
pub fn with_https<R>(logical: &mut Frame, work: impl for<'frame> FnOnce(&mut Https<'frame>) -> R) -> R {
    let mut http = Https::new(logical);
    let result = work(&mut http);
    drop(http);
    result
}

pub struct Https<'frame> {
    frame: NonNull<Frame>,
    client: *mut c_void,
    timer: *mut c_void,
    deadline: u32,
    lent: bool,
    kept: bool,
    sessions: u32,
    lowest: usize,
    time: NetworkTime,
    answered: bool,
    _exclusive: PhantomData<&'frame mut Frame>,
    _ui_task: PhantomData<*mut ()>,
}
impl<'frame> Https<'frame> {
    fn new(logical: &'frame mut Frame) -> Self {
        Self { frame: NonNull::from(logical), client: ptr::null_mut(), timer: ptr::null_mut(), deadline: 0,
            lent: false, kept: false, sessions: 0, lowest: usize::MAX,
            time: NetworkTime { seconds: 0, at_ms: 0 }, answered: false,
            _exclusive: PhantomData, _ui_task: PhantomData }
    }
    pub fn kept(&self) -> bool { self.kept }
    pub fn sessions(&self) -> u32 { self.sessions }
    pub fn frame_lowest(&self) -> usize { self.lowest }
    pub fn time(&self) -> NetworkTime { self.time }

    /// Frozen local UpdateHttps destruction boundary, before Wi-Fi teardown.
    /// The outer scope retains only frame permission and the result metadata.
    pub fn end_sessions(&mut self) { self.close(); self.stop_sweep(); }

    pub fn with_frame<R>(&mut self, work: impl for<'loan> FnOnce(Option<&'loan mut Frame>) -> R) -> R {
        if self.lent || self.kept { return work(None); }
        // SAFETY: this owner retains the exclusive original frame epoch. No
        // TLS block/hook is live, and this short loan cannot escape the closure.
        work(Some(unsafe { &mut *self.frame.as_ptr() }))
    }
    fn remaining(&self, clock: &mut dyn Clock) -> u32 {
        let left = self.deadline.wrapping_sub(clock.millis()) as i32;
        if !self.timer.is_null() && left > 0 { left as u32 } else { 0 }
    }
    fn close_client(&mut self) {
        if self.client.is_null() { return; }
        // SAFETY: exactly one live SDK handle; all per-call header contexts
        // have detached. Both actual calls run even if close reports failure.
        unsafe { native::nh_http_close(self.client); native::nh_http_cleanup(self.client); }
        self.client = ptr::null_mut();
    }
    fn stop_sweep(&mut self) {
        if self.timer.is_null() { return; }
        // SAFETY: this unique timer's callback contains only a deadline value,
        // never a pointer into Https; stop and delete precede handle release.
        unsafe { native::nh_timer_stop(self.timer); native::nh_timer_delete(self.timer); }
        self.timer = ptr::null_mut();
    }
}
impl Http for Https<'_> {
    fn limit(&mut self, milliseconds: u32, clock: &mut dyn Clock) {
        self.stop_sweep();
        self.deadline = clock.millis().wrapping_add(milliseconds);
        // SAFETY: the callback receives the u32 deadline by value, so even an
        // in-flight callback after timer deletion has no expired owner loan.
        self.timer = unsafe { native::nh_timer_create(sweep, self.deadline as usize as *mut c_void) };
        if !self.timer.is_null() {
            // SAFETY: create returned this live, uniquely owned SDK timer.
            if !unsafe { native::nh_timer_start(self.timer, 1_000_000) } { self.stop_sweep(); }
        }
    }
    fn open(&mut self, url: &CStr, api: bool, head: &mut HttpHead, location: &mut [u8; URL_BYTES], clock: &mut dyn Clock) -> bool {
        if self.kept { return false; }
        if !self.lent {
            if !tls::matches(self.frame.as_ptr()) { crate::app_power::stop(); }
            self.lent = tls::lend(self.frame.as_ptr());
            self.sessions = self.sessions.wrapping_add(u32::from(self.lent));
        }
        // Frozen base open closes only its client, retaining this TLS epoch.
        self.close_client();
        let left = self.remaining(clock);
        if left == 0 { return false; }
        // SAFETY: synchronous init copies URL into its SDK owner; config always
        // attaches the exact certificate bundle and disables native redirects.
        self.client = unsafe { native::nh_http_init(url.as_ptr(), left.min(10_000)) };
        if self.client.is_null() { return false; }
        // SAFETY: static C strings and the uniquely live client; preserve the
        // frozen short-circuit order of the two actual API header operations.
        if api && unsafe {
            !native::nh_http_set_header(self.client, c"Accept".as_ptr(), c"application/vnd.github+json".as_ptr()) ||
            !native::nh_http_set_header(self.client, c"X-GitHub-Api-Version".as_ptr(), c"2022-11-28".as_ptr())
        } { return false; }
        // SAFETY: one GET open on the valid, configured client, no body to write.
        if !unsafe { native::nh_http_open(self.client) } { return false; }
        let Some((length, cut)) = headers::fetch(self.client, head, location) else { return false; };
        // SAFETY: fetch ended and detached its stack callback, client stays live.
        head.status = unsafe { native::nh_http_status(self.client) };
        head.length = if length > 0 { length } else { -1 };
        let opened = head.status > 0 && !cut;
        if opened && !self.answered {
            self.answered = true;
            self.time = NetworkTime { seconds: head.date, at_ms: clock.millis() };
        }
        opened
    }
    fn read(&mut self, bytes: &mut [u8], clock: &mut dyn Clock) -> i32 {
        if self.client.is_null() || self.remaining(clock) == 0 { return -1; }
        // SAFETY: the short writable extent belongs to this one blocking read;
        // Core's read buffer fits the frozen SDK int-count ABI, no loan retained.
        let got = unsafe { native::nh_http_read(self.client, bytes.as_mut_ptr(), bytes.len() as i32) };
        if got != 0 { return if got > 0 { got } else { -1 }; }
        // SAFETY: only actual SDK-complete reception establishes successful EOF.
        if unsafe { native::nh_http_complete(self.client) } { 0 } else { -1 }
    }
    fn close(&mut self) {
        self.close_client();
        if !self.lent { return; }
        self.kept = !tls::reclaim(self.frame.as_ptr(), &mut self.lowest);
        self.lent = self.kept;
    }
}
impl Drop for Https<'_> {
    fn drop(&mut self) {
        self.end_sessions();
        // The caller's real restart normally handles kept before returning.
        // Never return the exclusive frame while any SDK allocation retains it.
        if self.kept { crate::app_power::stop(); }
    }
}

// SAFETY: the SDK invokes this static C callback with its copied deadline only;
// it neither borrows Https nor accesses a frame/client that may have closed.
unsafe extern "C" fn sweep(deadline: *mut c_void) {
    // SAFETY: scheduler, lwIP and Arduino clock remain alive for this SDK timer.
    unsafe {
        if (native::nb_millis().wrapping_sub(deadline as usize as u32) as i32) < 0 { return; }
        let first = native::nh_socket_first();
        for fd in first..first + native::nh_socket_count() { native::nh_socket_shutdown(fd); }
        native::nh_socket_sem_deinit();
    }
}
