// SPDX-License-Identifier: MIT
//! Frozen DEV action owner: one boot heap allocation, lexical observer loans.
#![forbid(unsafe_code)]
use newt_c3_board::MonotonicClock;
pub use newt_core::reader_action_profile::ActionStage;
#[cfg(feature = "dev-usb")]
use newt_core::{reader_action_profile::{ReaderActionProfile, CacheMissReason, trace_reader_action},
    reader_work_profile::ReaderOpenRoute};

#[cfg(feature = "dev-usb")]
pub type Owner = Option<newt_sdk_heap::Fixed<ReaderActionProfile>>;
#[cfg(not(feature = "dev-usb"))]
pub type Owner = ();

/// Called once immediately after Serial begins, as initializeDiagnostics was.
/// Refusal disables observations for this boot; no reader path retries it.
pub fn allocate() -> Owner {
    #[cfg(feature = "dev-usb")]
    { newt_sdk_heap::Fixed::try_new_with(ReaderActionProfile::new) }
}

#[derive(Clone, Copy)]
pub struct Trace<'a> {
    #[cfg(feature = "dev-usb")]
    owner: Option<&'a ReaderActionProfile>,
    #[cfg(feature = "dev-usb")]
    clock: MonotonicClock,
    #[cfg(not(feature = "dev-usb"))]
    borrow: core::marker::PhantomData<&'a ()>,
}
impl<'a> Trace<'a> {
    pub fn borrow(owner: &'a Owner, clock: MonotonicClock) -> Self {
        #[cfg(feature = "dev-usb")]
        { Self { owner: owner.as_deref(), clock } }
        #[cfg(not(feature = "dev-usb"))]
        { let _ = (owner, clock); Self { borrow: core::marker::PhantomData } }
    }
    pub fn begin(self) {
        #[cfg(feature = "dev-usb")]
        { let now = self.clock.micros(); if let Some(owner) = self.owner { owner.begin(now); } }
    }
    pub fn boundary(self, stage: u8) {
        #[cfg(feature = "dev-usb")]
        { let now = self.clock.micros(); if let Some(owner) = self.owner { owner.boundary(stage, now); } }
        #[cfg(not(feature = "dev-usb"))]
        let _ = stage;
    }
    pub fn scope(self, stage: ActionStage) -> Scope<'a> {
        self.boundary(stage as u8 | ActionStage::Push as u8);
        Scope(self)
    }
    pub fn finish(self) -> bool {
        #[cfg(feature = "dev-usb")]
        { let now = self.clock.micros(); self.owner.is_some_and(|owner| owner.finish(now)) }
        #[cfg(not(feature = "dev-usb"))]
        { false }
    }
    pub fn since(self, now: u32) -> Option<u32> {
        #[cfg(feature = "dev-usb")]
        { self.owner.map(|owner| owner.since(now)) }
        #[cfg(not(feature = "dev-usb"))]
        { let _ = now; None }
    }
    pub fn running(self) -> bool {
        #[cfg(feature = "dev-usb")]
        { self.owner.is_some_and(ReaderActionProfile::running) }
        #[cfg(not(feature = "dev-usb"))]
        { false }
    }
    /// Frozen loop tail: stop accounting before render-return publication,
    /// then emit the completed action after that publication.
    pub fn complete(self, log: &mut crate::serial_console::SerialConsole, returned: impl FnOnce()) {
        let timed = self.finish();
        returned();
        #[cfg(feature = "dev-usb")]
        if timed { self.emit(log); }
        #[cfg(not(feature = "dev-usb"))]
        let _ = (timed, log);
    }
    #[cfg(feature = "dev-usb")]
    pub fn emit(self, log: &mut crate::serial_console::SerialConsole) {
        if let Some(owner) = self.owner { trace_reader_action(log, owner); }
    }
    /// Existing work callbacks carry only the frozen scalar trace tags.
    pub fn event(self, event: &newt_core::admission::WorkEvent) -> bool {
        #[cfg(feature = "dev-usb")]
        {
        use newt_core::admission::WorkKind;
        match event.kind {
            WorkKind::ActionBoundary => { self.boundary(event.result as u8); true }
            WorkKind::OpenRoute => {
                self.route(event.result as u32); true
            }
            WorkKind::CacheMiss => { self.cache_miss(event.result as u8); true }
            _ => false,
        }
        }
        #[cfg(not(feature = "dev-usb"))]
        { let _ = (self, event); false }
    }
    pub fn route(self, route: u32) {
        #[cfg(feature = "dev-usb")]
        if let Some(owner) = self.owner {
            match route {
                value if value == ReaderOpenRoute::CacheProbe as u32 => owner.route(ReaderOpenRoute::CacheProbe),
                value if value == ReaderOpenRoute::CacheHit as u32 => owner.route(ReaderOpenRoute::CacheHit),
                value if value == ReaderOpenRoute::OrdinaryPage as u32 => owner.route(ReaderOpenRoute::OrdinaryPage),
                _ => {},
            }
        }
        #[cfg(not(feature = "dev-usb"))]
        let _ = route;
    }
    pub fn cache_miss(self, value: u8) {
        #[cfg(feature = "dev-usb")]
        if let Some(owner) = self.owner {
            let reason = match value {
                0 => CacheMissReason::None, 1 => CacheMissReason::Lease,
                2 => CacheMissReason::Attach, 3 => CacheMissReason::Open,
                4 => CacheMissReason::Index, 5 => CacheMissReason::Segment,
                6 => CacheMissReason::Range, 7 => CacheMissReason::Read,
                8 => CacheMissReason::CharStart, 9 => CacheMissReason::Image,
                10 => CacheMissReason::Finish, 11 => CacheMissReason::Capture,
                12 => CacheMissReason::Map, 13 => CacheMissReason::Edge,
                _ => return,
            };
            owner.cache_miss(reason);
        }
        #[cfg(not(feature = "dev-usb"))]
        let _ = value;
    }
}
pub struct Scope<'a>(Trace<'a>);
impl Drop for Scope<'_> {
    fn drop(&mut self) { self.0.boundary(ActionStage::Restore as u8); }
}

/// Preserve the exit operation's existing cancellation policy while lending
/// its action observations through the same native authority boundary.
pub(crate) struct ObservedAuthority<'a> {
    pub(crate) original: &'a dyn newt_native_storage::Authority,
    pub(crate) trace: Trace<'a>,
}
impl newt_native_storage::Authority for ObservedAuthority<'_> {
    fn cancelled(&self) -> bool { self.original.cancelled() }
    fn invalid(&self) -> bool { self.original.invalid() }
    fn failure(&self) { self.original.failure(); }
    fn action_boundary(&self, stage: u8) { self.trace.boundary(stage); }
    fn open_route(&self, route: u32) { self.trace.route(route); }
    fn cache_miss(&self, reason: u8) { self.trace.cache_miss(reason); }
}
