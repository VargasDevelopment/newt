// SPDX-License-Identifier: MIT
//! Stateless adapter fixtures replace synchronous path leaves without SDK I/O.
extern crate std;

use core::cell::Cell;
use super::*;

#[derive(Clone, Copy, Default)]
struct Call {
    count: usize,
    from: usize,
    to: usize,
    tag: i32,
}
std::thread_local! {
    static CALL: Cell<Call> = Cell::new(Call::default());
}

// SAFETY: host-only replacement of the synchronous FFI leaf. No SDK access or
// pointer dereference; record identity without retaining a usable pointer.
pub(super) unsafe extern "C" fn newt_io_rename(from: *const c_char, to: *const c_char) -> i32 {
    CALL.with(|state| {
        let call = state.get();
        state.set(Call { count: call.count + 1, from: from as usize, to: to as usize, ..call });
        call.tag
    })
}

#[test]
fn borrowed_rename_preserves_buffers_single_call_and_result() {
    let mut native = OsNative { _owner: PhantomData };
    let from = c"cache/temporary";
    let to = c"cache/final";
    for (tag, expected) in [
        (0, Ok(())),
        (1, Err(Error::Interrupted)),
        (2, Err(Error::Missing)),
        (3, Err(Error::Exists)),
        (4, Err(Error::Cancelled)),
        (6, Err(Error::Unavailable)),
        (99, Err(Error::Other)),
    ] {
        CALL.with(|state| state.set(Call { tag, ..Call::default() }));
        assert_eq!(native.rename_cstr(from, to), expected);
        CALL.with(|state| {
            let call = state.get();
            assert_eq!(call.count, 1);
            assert_eq!(call.from, from.as_ptr() as usize);
            assert_eq!(call.to, to.as_ptr() as usize);
        });
    }
}

#[test]
fn borrowed_rename_enforces_utf8_and_path_limit_before_native_call() {
    let mut native = OsNative { _owner: PhantomData };
    let mut maximum = [b'a'; MAX_PATH_BYTES + 1];
    maximum[MAX_PATH_BYTES] = 0;
    let maximum = CStr::from_bytes_with_nul(&maximum).unwrap();
    CALL.with(|state| state.set(Call::default()));
    assert_eq!(native.rename_cstr(maximum, maximum), Ok(()));
    CALL.with(|state| {
        let call = state.get();
        assert_eq!(call.count, 1);
        assert_eq!(call.from, maximum.as_ptr() as usize);
        assert_eq!(call.to, maximum.as_ptr() as usize);
    });

    let mut oversized = [b'a'; MAX_PATH_BYTES + 2];
    oversized[MAX_PATH_BYTES + 1] = 0;
    let oversized = CStr::from_bytes_with_nul(&oversized).unwrap();
    let invalid_utf8 = CStr::from_bytes_with_nul(b"\xff\0").unwrap();
    CALL.with(|state| state.set(Call::default()));
    for invalid in [oversized, invalid_utf8] {
        assert_eq!(native.rename_cstr(invalid, c"valid"), Err(Error::Other));
        assert_eq!(native.rename_cstr(c"valid", invalid), Err(Error::Other));
    }
    CALL.with(|state| assert_eq!(state.get().count, 0));
}

#[test]
fn path_scope_encodes_valid_paths_and_rejects_invalid_paths_before_loan() {
    let maximum = [b'a'; MAX_PATH_BYTES];
    let maximum = core::str::from_utf8(&maximum).unwrap();
    for input in ["", "library/é", maximum] {
        assert_eq!(Path::with(input, |path| {
            assert_eq!(path.as_c_str()?.to_bytes(), input.as_bytes());
            Ok(input.len())
        }), Ok(input.len()));
    }

    let oversized = [b'a'; MAX_PATH_BYTES + 1];
    let oversized = core::str::from_utf8(&oversized).unwrap();
    for invalid in ["embedded\0nul", oversized] {
        let mut called = false;
        assert_eq!(Path::with(invalid, |_| {
            called = true;
            Ok(())
        }), Err(Error::Other));
        assert!(!called);
    }
}

#[test]
fn legacy_rename_preserves_single_call_results_and_rejects_invalid_names() {
    let mut native = OsNative { _owner: PhantomData };
    for (tag, expected) in [(0, Ok(())), (2, Err(Error::Missing))] {
        CALL.with(|state| state.set(Call { tag, ..Call::default() }));
        assert_eq!(native.rename("cache/temporary", "cache/final"), expected);
        CALL.with(|state| assert_eq!(state.get().count, 1));
    }

    let oversized = [b'a'; MAX_PATH_BYTES + 1];
    let oversized = core::str::from_utf8(&oversized).unwrap();
    CALL.with(|state| state.set(Call::default()));
    for invalid in ["embedded\0nul", oversized] {
        assert_eq!(native.rename(invalid, "valid"), Err(Error::Other));
        assert_eq!(native.rename("valid", invalid), Err(Error::Other));
    }
    CALL.with(|state| assert_eq!(state.get().count, 0));
}

// Exercise every other Path scope through its actual adapter, without SDK I/O.
mod path_users {
    use super::super::*;
    use super::std::{self, cell::{Cell, RefCell}, string::{String, ToString}, vec::Vec};

    std::thread_local! {
        static TAG: Cell<i32> = const { Cell::new(0) };
        static CALLS: RefCell<Vec<(&'static str, String, u32)>> = const { RefCell::new(Vec::new()) };
    }

    // SAFETY: only called synchronously with the adapter's live terminated path.
    unsafe fn record(op: &'static str, path: *const c_char, argument: u32) -> i32 {
        // SAFETY: the fixture caller supplies that live path, consumed here only.
        let path = unsafe { CStr::from_ptr(path) }.to_str().unwrap().to_string();
        CALLS.with(|calls| calls.borrow_mut().push((op, path, argument)));
        TAG.with(Cell::get)
    }

    // SAFETY: host leaf replacement; the adapter lends the writable fd output.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn newt_io_open(path: *const c_char, mode: u32, out: *mut i32) -> i32 {
        // SAFETY: supplied path is live; no pointer is retained.
        let tag = unsafe { record("open", path, mode) };
        if tag == 0 {
            // SAFETY: the adapter provides one writable scalar, not an SDK fd.
            unsafe { *out = 17; }
        }
        tag
    }

    // SAFETY: host replacement with the exact adapter-owned stat output ABI.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn newt_io_stat(path: *const c_char, follow: u32, out: *mut WireStat) -> i32 {
        // SAFETY: supplied path is live throughout this synchronous call.
        let tag = unsafe { record("stat", path, follow) };
        if tag == 0 {
            // SAFETY: the adapter exclusively lends this complete output value.
            unsafe { *out = WireStat { size: 8192, kind: 1, ..WireStat::default() }; }
        }
        tag
    }

    macro_rules! path_leaf {
        ($name:ident, $op:literal) => {
            // SAFETY: host leaf replacement consumes the live path synchronously.
            #[unsafe(no_mangle)]
            unsafe extern "C" fn $name(path: *const c_char) -> i32 {
                // SAFETY: the adapter lends a terminated path and no pointer escapes.
                unsafe { record($op, path, 0) }
            }
        };
    }
    path_leaf!(newt_io_mkdir, "mkdir");
    path_leaf!(newt_io_unlink, "unlink");
    path_leaf!(newt_io_rmdir, "rmdir");

    // SAFETY: the host sentinel represents no resource and is never dereferenced.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn newt_io_opendir(path: *const c_char, out: *mut *mut c_void) -> i32 {
        // SAFETY: the adapter lends the path for this call only.
        let tag = unsafe { record("opendir", path, 0) };
        if tag == 0 {
            // SAFETY: output is exclusive; its sentinel is consumed only by close.
            unsafe { *out = NonNull::<u8>::dangling().as_ptr().cast(); }
        }
        tag
    }

    // SAFETY: consumes only the fixture sentinel, with no SDK/resource access.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn newt_io_closedir(dir: *mut c_void) -> i32 {
        assert_eq!(dir, NonNull::<u8>::dangling().as_ptr().cast());
        0
    }

    fn invoke(native: &mut OsNative<'_>, op: &str, path: &str) -> Result<()> {
        match op {
            "open" => { assert_eq!(native.open(path, Open::CreateTruncateWriteOnly)?, 17); }
            "stat" => {
                let stat = native.stat(path, true)?;
                assert_eq!((stat.kind, stat.size), (Kind::Regular, 8192));
            }
            "mkdir" => native.mkdir(path)?,
            "unlink" => native.unlink(path)?,
            "rmdir" => native.rmdir(path)?,
            "opendir" => { let dir = native.open_dir(path)?; native.close_dir(dir)?; }
            _ => panic!("fixture operation"),
        }
        Ok(())
    }

    #[test]
    fn path_scope_users_preserve_calls_arguments_errors_and_validation_order() {
        let mut native = OsNative { _owner: PhantomData };
        let maximum = "a".repeat(MAX_PATH_BYTES);
        let oversized = "a".repeat(MAX_PATH_BYTES + 1);
        for (op, argument) in [("open", 6), ("stat", 1), ("mkdir", 0),
            ("unlink", 0), ("rmdir", 0), ("opendir", 0)] {
            for path in ["", "library/é", maximum.as_str()] {
                for (tag, expected) in [(0, Ok(())), (1, Err(Error::Interrupted)),
                    (2, Err(Error::Missing)), (3, Err(Error::Exists)),
                    (4, Err(Error::Cancelled)), (6, Err(Error::Unavailable)),
                    (99, Err(Error::Other))] {
                    TAG.with(|state| state.set(tag));
                    CALLS.with(|calls| calls.borrow_mut().clear());
                    assert_eq!(invoke(&mut native, op, path), expected);
                    CALLS.with(|calls| assert_eq!(*calls.borrow(), [(op, path.to_string(), argument)]));
                }
            }
            CALLS.with(|calls| calls.borrow_mut().clear());
            for path in ["embedded\0nul", oversized.as_str()] {
                assert_eq!(invoke(&mut native, op, path), Err(Error::Other));
            }
            CALLS.with(|calls| assert!(calls.borrow().is_empty()));
        }
    }
}
