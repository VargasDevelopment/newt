//! Safe policy for the pinned native FilePort. See HANDOFF.md for caller domain.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
pub mod reading_state;
pub mod sd_source;
pub mod source_file;
mod port;
mod route;
mod text;
pub use port::*;
pub use route::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Interrupted,
    Missing,
    Exists,
    Cancelled,
    /// EIO, ENODEV or ENXIO: the medium itself is missing or failed. Only the
    /// reading-state store classifies it; every other caller treats it as Other.
    Unavailable,
    Other,
}
pub type Result<T> = core::result::Result<T, Error>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Regular,
    Directory,
    Other,
}
#[derive(Clone, Copy, Debug)]
pub struct Stat {
    pub kind: Kind,
    pub size: i64,
    pub inode: u64,
    pub device: u64,
    pub seconds: i64,
    pub nanos: u32,
}
#[derive(Clone, Copy, Debug)]
pub enum Open {
    /// O_RDONLY source open, without the cache port's O_NONBLOCK flag.
    SourceReadOnly,
    ReadNonblock,
    ReadWriteNonblock,
    CreateExclusive,
    /// O_RDWR, the reading-state store's writable open.
    ReadWrite,
    /// O_WRONLY|O_CREAT|O_EXCL, mode 0600: the reading-state repair evidence copy.
    CreateExclusiveWriteOnly,
    /// O_WRONLY|O_CREAT|O_TRUNC, mode 0600: the Library list/run writer.
    CreateTruncateWriteOnly,
}
/// Native fd ownership belongs exclusively to the port until close is called.
/// One syscall per method, no hidden retries. Preserve returned success even
/// when cancellation arrives during it. A failed rename may have taken effect.
/// Paths are UTF-8, NUL-free, without a terminator. Adapters supply native flags.
/// Implementations cannot reenter the currently borrowed port; see HANDOFF.md.
pub trait Native {
    fn open(&mut self, path: &str, mode: Open) -> Result<i32>;
    fn close(&mut self, fd: i32) -> Result<()>;
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize>;
    fn write(&mut self, fd: i32, input: &[u8]) -> Result<usize>;
    fn seek(&mut self, fd: i32, offset: u32) -> Result<u64>;
    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()>;
    fn sync(&mut self, fd: i32) -> Result<()>;
    fn fstat(&mut self, fd: i32) -> Result<Stat>;
    fn stat(&mut self, path: &str, no_follow: bool) -> Result<Stat>;
    fn mkdir(&mut self, path: &str) -> Result<()>;
    fn unlink(&mut self, path: &str) -> Result<()>;
    fn rename(&mut self, from: &str, to: &str) -> Result<()>;
    /// Borrowed terminated paths; adapters may avoid copying their backing.
    /// Preserve the UTF-8 contract and the existing single rename call.
    fn rename_cstr(&mut self, from: &core::ffi::CStr, to: &core::ffi::CStr) -> Result<()> {
        let from = from.to_str().map_err(|_| Error::Other)?;
        let to = to.to_str().map_err(|_| Error::Other)?;
        self.rename(from, to)
    }
    /// Shared native process nonce; wrapping increment, first value is one.
    fn next_nonce(&mut self) -> u32;
}
/// Directory listing, apart from Native so FilePort's implementors need not
/// list. One native call per method, no hidden retries or drop-time I/O: the
/// caller closes every listing it opened. Names are the entry's bytes.
pub trait Directory {
    type Listing;
    fn open_dir(&mut self, path: &str) -> Result<Self::Listing>;
    /// Ok(None) at the end. Ok(Some(n)): the entry's full name is n bytes, of
    /// which the first min(n, name.len()) were stored in `name`.
    fn read_dir(&mut self, listing: &mut Self::Listing, name: &mut [u8]) -> Result<Option<usize>>;
    fn close_dir(&mut self, listing: Self::Listing) -> Result<()>;
}
/// No callbacks may mutate/reenter this port. Interior state can revoke an
/// authority during a syscall, and is observed at the next boundary.
pub trait Authority {
    fn cancelled(&self) -> bool;
    fn action_boundary(&self, _boundary: u8) {}
    fn open_route(&self, _route: u32) {}
    fn cache_miss(&self, _reason: u8) {}
    fn invalid(&self) -> bool {
        false
    }
    fn failure(&self) {}
}

/// Attempt-owned facts, retained through explicit cleanup and after detach.
/// Callbacks have no access to these cells through the policy API.
pub struct ProducerPolicy<'a> {
    authority: &'a dyn Authority,
    failed: core::cell::Cell<bool>,
    cancellation_observed: core::cell::Cell<bool>,
}
impl<'a> ProducerPolicy<'a> {
    pub fn new(authority: &'a dyn Authority) -> Self {
        Self {
            authority,
            failed: core::cell::Cell::new(false),
            cancellation_observed: core::cell::Cell::new(false),
        }
    }
    pub fn failed(&self) -> bool {
        self.failed.get()
    }
    pub fn cancellation_observed(&self) -> bool {
        self.cancellation_observed.get()
    }
    /// The owner may classify its own work before port cleanup.
    pub fn record_failure(&self) {
        if !self.cancellation_observed.get() {
            self.failed.set(true);
            self.authority.failure();
        }
    }
    pub fn boundary(&self) -> bool {
        if self.failed.get() || self.cancellation_observed.get() {
            return false;
        }
        if self.authority.cancelled() {
            self.cancellation_observed.set(true);
            false
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        cell::Cell,
        sync::atomic::{AtomicU32, Ordering},
    };
    struct Cancel(Cell<bool>);
    impl Authority for Cancel {
        fn cancelled(&self) -> bool {
            self.0.get()
        }
    }
    #[test]
    fn failure_precedes_unobserved_cancellation() {
        let cancel = Cancel(Cell::new(true));
        let policy = ProducerPolicy::new(&cancel);
        policy.record_failure();
        assert!(!policy.boundary());
        cancel.0.set(false);
        assert!(policy.failed());
        assert!(!policy.cancellation_observed());
        assert!(!policy.boundary());
    }
    #[test]
    fn observed_cancellation_survives_clear_and_suppresses_cleanup_failure() {
        let cancel = Cancel(Cell::new(true));
        let policy = ProducerPolicy::new(&cancel);
        assert!(!policy.boundary());
        cancel.0.set(false);
        policy.record_failure();
        assert!(!policy.failed());
        assert!(policy.cancellation_observed());
        assert!(!policy.boundary());
    }
    #[test]
    fn nested_budget_keeps_attempts_and_wrapping_deadline() {
        static NOW: AtomicU32 = AtomicU32::new(0);
        let start = u32::MAX - 255;
        NOW.store(start, Ordering::Relaxed);
        let cancel = Cancel(Cell::new(false));
        let route_state = RouteState::new(
            "cache",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );
        let route = route_state.lend(&cancel, || NOW.load(Ordering::Relaxed));
        assert!(route.begin_cache_read(2, 1));
        assert!(route.step());
        assert!(route.begin_cache_read(100, 100));
        assert!(!route.step());
        assert!(!route.cache_read_size(3));
        NOW.store(start.wrapping_add(2999), Ordering::Relaxed);
        assert!(route.finish_cache_read());
        NOW.store(start.wrapping_add(3000), Ordering::Relaxed);
        assert!(!route.cache_read_live());
        assert!(!route.finish_cache_read());
        assert!(route.cache_read_live());
        assert!(route.begin_cache_read(3, 1));
        assert!(route.step());
        assert!(route.finish_cache_read());
    }
}

pub mod page_entry_census;

pub mod page_entries;
