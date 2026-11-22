// MIT. Unsafe is confined to the synchronous native bridge and nonce exclusion.
#![no_std]
use core::{
    ffi::{c_char, c_void, CStr},
    marker::PhantomData,
    ptr::NonNull,
};
use newt_native_storage::{Directory, Error, Kind, Native, Open, Result, Stat};
mod library_folders;
pub use library_folders::LibraryFolders;
mod library_files;
pub use library_files::LibraryFiles;
mod purge;
mod posix_file;
mod posix_directory;
mod update_files;
mod update_directory;
mod crossink_files;
pub use crossink_files::CrossInkFiles;
mod optional_scope;
pub mod raw_download;
pub use update_files::UpdateFiles;
pub use update_directory::UpdateDirectory;
pub mod cover;
#[repr(C)]
#[derive(Default)]
struct WireStat {
    size: i64,
    inode: u64,
    device: u64,
    seconds: i64,
    nanos: u32,
    kind: u32,
}
const _: () = {
    assert!(core::mem::size_of::<WireStat>() == 40);
    assert!(core::mem::offset_of!(WireStat, size) == 0);
    assert!(core::mem::offset_of!(WireStat, inode) == 8);
    assert!(core::mem::offset_of!(WireStat, device) == 16);
    assert!(core::mem::offset_of!(WireStat, seconds) == 24);
    assert!(core::mem::offset_of!(WireStat, nanos) == 32);
    assert!(core::mem::offset_of!(WireStat, kind) == 36);
};
unsafe extern "C" {
    fn newt_io_open(path: *const c_char, mode: u32, out: *mut i32) -> i32;
    fn newt_io_close(fd: i32) -> i32;
    fn newt_io_read(fd: i32, data: *mut u8, bytes: usize, out: *mut usize) -> i32;
    fn newt_io_write(fd: i32, data: *const u8, bytes: usize, out: *mut usize) -> i32;
    fn newt_io_seek(fd: i32, at: u32, out: *mut u64) -> i32;
    fn newt_io_truncate(fd: i32, bytes: u32) -> i32;
    fn newt_io_sync(fd: i32) -> i32;
    fn newt_io_fstat(fd: i32, out: *mut WireStat) -> i32;
    fn newt_io_stat(path: *const c_char, no_follow: u32, out: *mut WireStat) -> i32;
    fn newt_io_mkdir(path: *const c_char) -> i32;
    fn newt_io_unlink(path: *const c_char) -> i32;
    #[cfg(not(test))]
    fn newt_io_rename(from: *const c_char, to: *const c_char) -> i32;
    fn newt_io_opendir(path: *const c_char, out: *mut *mut c_void) -> i32;
    fn newt_io_readdir(
        dir: *mut c_void,
        name: *mut c_char,
        capacity: usize,
        length: *mut usize,
        more: *mut u32,
    ) -> i32;
    fn newt_io_closedir(dir: *mut c_void) -> i32;
}
fn status(tag: i32) -> Result<()> {
    match tag {
        0 => Ok(()),
        1 => Err(Error::Interrupted),
        2 => Err(Error::Missing),
        3 => Err(Error::Exists),
        4 => Err(Error::Cancelled),
        6 => Err(Error::Unavailable),
        _ => Err(Error::Other),
    }
}
impl From<WireStat> for Stat {
    fn from(s: WireStat) -> Self {
        Self {
            kind: match s.kind {
                1 => Kind::Regular,
                2 => Kind::Directory,
                _ => Kind::Other,
            },
            size: s.size,
            inode: s.inode,
            device: s.device,
            seconds: s.seconds,
            nanos: s.nanos,
        }
    }
}
/// Maximum path bytes excluding NUL; representation failure returns Other.
pub const MAX_PATH_BYTES: usize = 511;
struct Path([u8; MAX_PATH_BYTES + 1]);
impl Path {
    // Keep the encoded path in this synchronous scope rather than moving it.
    #[inline(never)]
    fn with<R>(path: &str, work: impl FnOnce(&Path) -> Result<R>) -> Result<R> {
        if path.len() > MAX_PATH_BYTES || path.as_bytes().contains(&0) {
            return Err(Error::Other);
        }
        let mut out = Self([0; MAX_PATH_BYTES + 1]);
        out.0[..path.len()].copy_from_slice(path.as_bytes());
        work(&out)
    }
    fn as_ptr(&self) -> *const c_char {
        self.0.as_ptr().cast()
    }
    fn as_c_str(&self) -> Result<&CStr> {
        CStr::from_bytes_until_nul(&self.0).map_err(|_| Error::Other)
    }
}
/// Stateless backend: raw descriptor lifetime belongs to the policy owner.
///
/// SAFETY of all calls below: link native_io.c for this exact target ABI.
/// It accesses only supplied lengths, writes outputs only on success, returns
/// synchronously and retains no pointer. Paths are terminated; slice and scalar
/// outputs remain live and uniquely borrowed until return. Zero-length pointers
/// must not be dereferenced. The separately documented page-entry walk alone
/// lends one synchronous callback, as does the separate FAT Library scope;
/// other calls never call back or retry.
/// UI-task authority, not a descriptor owner. Never Send/Sync or ambient.
/// Shared views serialize on the acquiring UI task. The page-entry visitor
/// gets one short native reborrow after each directory read has returned.
/// Library traversal similarly calls native methods only between FAT calls.
pub struct UiIo {
    _ui: PhantomData<*mut ()>,
}
impl UiIo {
    /// # Safety
    /// Acquire only inside the sole platform owner's synchronous scope. The
    /// fixed X3 profile, initialized shared SPI and mounted FAT VFS must remain
    /// stable. The same UI owner may perform blocking display/Wire operations
    /// between synchronous bridge calls, with every parse/frame loan returned.
    /// Exclude display/power/profile reentry, competing tasks, external descriptor
    /// users and async bus work until all views end. Only the reviewed page-entry
    /// callback may reborrow its same native view after a directory read ends;
    /// the Library scope lends its FAT state between synchronous native calls.
    /// This does not acquire an SDK lock or mount a card.
    pub unsafe fn acquire() -> Self {
        Self { _ui: PhantomData }
    }
    /// Each view moves into an EXISTING descriptor owner. No pointer is stored.
    pub fn port(&self) -> OsNative<'_> {
        OsNative {
            _owner: PhantomData,
        }
    }
}
pub struct OsNative<'ui> {
    _owner: PhantomData<&'ui UiIo>,
}
fn open_mode(mode: Open) -> u32 {
    match mode {
        Open::SourceReadOnly => 0,
        Open::ReadNonblock => 1,
        Open::ReadWriteNonblock => 2,
        Open::CreateExclusive => 3,
        Open::ReadWrite => 4,
        Open::CreateExclusiveWriteOnly => 5,
        Open::CreateTruncateWriteOnly => 6,
    }
}
impl Native for OsNative<'_> {
    fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
        Path::with(path, |path| self.open_cstr(path.as_c_str()?, mode))
    }
    fn close(&mut self, fd: i32) -> Result<()> {
        status(unsafe { newt_io_close(fd) })
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> {
        let mut got = 0;
        status(unsafe { newt_io_read(fd, out.as_mut_ptr(), out.len(), &mut got) })?;
        Ok(got)
    }
    fn write(&mut self, fd: i32, input: &[u8]) -> Result<usize> {
        let mut got = 0;
        status(unsafe { newt_io_write(fd, input.as_ptr(), input.len(), &mut got) })?;
        Ok(got)
    }
    fn seek(&mut self, fd: i32, at: u32) -> Result<u64> {
        let mut out = 0;
        status(unsafe { newt_io_seek(fd, at, &mut out) })?;
        Ok(out)
    }
    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()> {
        status(unsafe { newt_io_truncate(fd, bytes) })
    }
    fn sync(&mut self, fd: i32) -> Result<()> {
        status(unsafe { newt_io_sync(fd) })
    }
    fn fstat(&mut self, fd: i32) -> Result<Stat> {
        let mut out = WireStat::default();
        status(unsafe { newt_io_fstat(fd, &mut out) })?;
        Ok(out.into())
    }
    fn stat(&mut self, path: &str, no_follow: bool) -> Result<Stat> {
        Path::with(path, |path| {
            let mut out = WireStat::default();
            status(unsafe { newt_io_stat(path.as_ptr(), u32::from(no_follow), &mut out) })?;
            Ok(out.into())
        })
    }
    fn mkdir(&mut self, path: &str) -> Result<()> {
        Path::with(path, |path| status(unsafe { newt_io_mkdir(path.as_ptr()) }))
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        Path::with(path, |path| status(unsafe { newt_io_unlink(path.as_ptr()) }))
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        Path::with(from, |from| {
            Path::with(to, |to| self.rename_cstr(from.as_c_str()?, to.as_c_str()?))
        })
    }
    fn rename_cstr(&mut self, from: &CStr, to: &CStr) -> Result<()> {
        if from.to_bytes().len() > MAX_PATH_BYTES || to.to_bytes().len() > MAX_PATH_BYTES {
            return Err(Error::Other);
        }
        from.to_str().map_err(|_| Error::Other)?;
        to.to_str().map_err(|_| Error::Other)?;
        status(unsafe { newt_io_rename(from.as_ptr(), to.as_ptr()) })
    }
    fn next_nonce(&mut self) -> u32 {
        nonce::next()
    }
}
#[cfg(test)]
mod rename_tests;
#[cfg(test)]
use rename_tests::newt_io_rename;
/// One open native DIR*. Only close_dir releases it: no drop-time I/O, and
/// the pointer keeps it !Send and !Sync, on the acquiring UI task.
pub struct OsListing<'ui> {
    dir: NonNull<c_void>,
    _owner: PhantomData<&'ui UiIo>,
}
impl<'ui> Directory for OsNative<'ui> {
    type Listing = OsListing<'ui>;
    fn open_dir(&mut self, path: &str) -> Result<OsListing<'ui>> {
        Path::with(path, |path| {
            let mut out = core::ptr::null_mut();
            status(unsafe { newt_io_opendir(path.as_ptr(), &mut out) })?;
            let dir = NonNull::new(out).ok_or(Error::Other)?;
            Ok(OsListing {
                dir,
                _owner: PhantomData,
            })
        })
    }
    fn read_dir(&mut self, listing: &mut OsListing<'ui>, name: &mut [u8]) -> Result<Option<usize>> {
        let (mut length, mut more) = (0, 0);
        // SAFETY: the DIR* is live until close_dir consumes the listing.
        status(unsafe {
            newt_io_readdir(
                listing.dir.as_ptr(),
                name.as_mut_ptr().cast(),
                name.len(),
                &mut length,
                &mut more,
            )
        })?;
        Ok((more != 0).then_some(length))
    }
    fn close_dir(&mut self, listing: OsListing<'ui>) -> Result<()> {
        // SAFETY: consuming the listing makes this the DIR*'s only release.
        status(unsafe { newt_io_closedir(listing.dir.as_ptr()) })
    }
}
#[cfg(target_has_atomic = "32")]
mod nonce {
    use core::sync::atomic::{AtomicU32, Ordering};
    static NONCE: AtomicU32 = AtomicU32::new(0);
    pub fn next() -> u32 {
        NONCE.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    }
}
#[cfg(all(not(target_has_atomic = "32"), target_arch = "riscv32"))]
mod nonce {
    use core::cell::UnsafeCell;
    struct Counter(UnsafeCell<u32>);
    // SAFETY: every access holds the actual single-core C3 SDK exclusion.
    unsafe impl Sync for Counter {}
    static NONCE: Counter = Counter(UnsafeCell::new(0));
    unsafe extern "C" {
        fn newt_io_nonce_enter();
        fn newt_io_nonce_exit();
    }
    pub fn next() -> u32 {
        // SAFETY: task context after SDK scheduler startup; no callback, panic
        // or syscall while held. C requires CONFIG_FREERTOS_UNICORE on C3.
        unsafe {
            newt_io_nonce_enter();
            let next = (*NONCE.0.get()).wrapping_add(1);
            *NONCE.0.get() = next;
            newt_io_nonce_exit();
            next
        }
    }
}
#[cfg(all(not(target_has_atomic = "32"), not(target_arch = "riscv32")))]
compile_error!("Nonce needs 32-bit atomics or the reviewed X3 SDK exclusion");


mod page_entries;
