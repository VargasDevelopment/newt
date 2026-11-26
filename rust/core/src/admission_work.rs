//! Short work observations share the existing scheduler callback pointer.
use super::{WorkEvent, Yield};
#[cfg(feature = "dev-usb")]
use super::WorkKind;
use crate::{reader_action_profile::{ActionStage, CacheMissReason},
    reader_work_profile::ReaderOpenRoute};

/// Scalar action observations reuse the existing operation's borrowed hook.
#[inline]
pub fn action_boundary(hook: Yield<'_>, boundary: u8) {
    #[cfg(feature = "dev-usb")]
    if let Some(hook) = hook { hook.observe(WorkEvent::new(WorkKind::ActionBoundary, i32::from(boundary))); }
    #[cfg(not(feature = "dev-usb"))]
    let _ = (hook, boundary);
}
#[inline]
pub fn open_route(hook: Yield<'_>, route: ReaderOpenRoute) {
    #[cfg(feature = "dev-usb")]
    if let Some(hook) = hook { hook.observe(WorkEvent::new(WorkKind::OpenRoute, route as i32)); }
    #[cfg(not(feature = "dev-usb"))]
    let _ = (hook, route);
}
#[inline]
pub fn cache_miss(hook: Yield<'_>, reason: CacheMissReason) {
    #[cfg(feature = "dev-usb")]
    if let Some(hook) = hook { hook.observe(WorkEvent::new(WorkKind::CacheMiss, reason as i32)); }
    #[cfg(not(feature = "dev-usb"))]
    let _ = (hook, reason);
}
#[inline]
pub fn layout_begin(hook: Yield<'_>) {
    #[cfg(feature = "page-turn-trace")]
    if let Some(hook) = hook { hook.observe(WorkEvent::new(super::WorkKind::LayoutBegin, 0)); }
    #[cfg(not(feature = "page-turn-trace"))]
    let _ = hook;
}
#[inline]
pub fn layout_end(hook: Yield<'_>, pages: u32) {
    #[cfg(feature = "page-turn-trace")]
    if let Some(hook) = hook { hook.observe(WorkEvent::new(super::WorkKind::LayoutEnd, pages as i32)); }
    #[cfg(not(feature = "page-turn-trace"))]
    let _ = (hook, pages);
}

/// Restores the suspended stage on every return, using the same short hook.
pub struct ActionScope<'a> {
    #[cfg(feature = "dev-usb")]
    hook: Yield<'a>,
    #[cfg(not(feature = "dev-usb"))]
    unused: core::marker::PhantomData<&'a ()>,
}
impl<'a> ActionScope<'a> {
    pub const fn inactive() -> Self {
        Self {
            #[cfg(feature = "dev-usb")] hook: None,
            #[cfg(not(feature = "dev-usb"))] unused: core::marker::PhantomData,
        }
    }
    #[inline]
    pub fn new(hook: Yield<'a>, stage: ActionStage) -> Self {
        action_boundary(hook, stage as u8 | ActionStage::Push as u8);
        Self {
            #[cfg(feature = "dev-usb")] hook,
            #[cfg(not(feature = "dev-usb"))] unused: core::marker::PhantomData,
        }
    }
}
impl Drop for ActionScope<'_> {
    fn drop(&mut self) {
        #[cfg(feature = "dev-usb")]
        action_boundary(self.hook, ActionStage::Restore as u8);
    }
}
#[cfg(all(feature = "dev-usb", target_pointer_width = "32"))]
const _: () = assert!(core::mem::size_of::<ActionScope<'static>>() == 8);
#[cfg(not(feature = "dev-usb"))]
const _: () = assert!(core::mem::size_of::<ActionScope<'static>>() == 0);

/// A legacy closure only yields. An operation may also observe work without
/// installing another callback or retaining its profile in the reader owner.
pub trait WorkYield {
    fn yield_now(&self);
    fn observe(&self, _event: WorkEvent) {}
}

impl<F: Fn() + ?Sized> WorkYield for F {
    fn yield_now(&self) { self(); }
}

pub struct ScopedWork<'a> {
    yield_hook: &'a dyn Fn(),
    observer: Option<&'a dyn Fn(WorkEvent)>,
}
impl<'a> ScopedWork<'a> {
    pub fn new(yield_hook: &'a dyn Fn(), observer: Option<&'a dyn Fn(WorkEvent)>) -> Self {
        Self { yield_hook, observer }
    }
}
impl WorkYield for ScopedWork<'_> {
    fn yield_now(&self) { (self.yield_hook)(); }
    fn observe(&self, event: WorkEvent) {
        if let Some(observer) = self.observer { observer(event); }
    }
}

const _: () = assert!(core::mem::size_of::<super::Yield<'static>>()
    == core::mem::size_of::<Option<&'static dyn Fn()>>());
