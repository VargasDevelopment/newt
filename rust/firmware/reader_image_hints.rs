// SPDX-License-Identifier: MIT
//! The existing source-hints record; stat equality never grants full-D authority.
use newt_native_storage::{Kind, Native, Route, ReaderSourceHints, ReaderSourceHintsState,
    SourceChangeHints};
pub(super) fn current(native: &mut impl Native, path: &str) -> SourceChangeHints {
    match native.stat(path, false) {
        Ok(info) if info.kind == Kind::Regular && info.size > 0 =>
            SourceChangeHints { bytes: info.size as u64, modified: info.seconds },
        _ => SourceChangeHints::default(),
    }
}
pub(super) fn matching(native: &mut impl Native, route: &Route<'_>, digest: &[u8; 32],
    current: SourceChangeHints,
) -> bool {
    if !current.valid() { return false; }
    let mut state = ReaderSourceHintsState::new(native, route);
    let mut previous = SourceChangeHints::default();
    let matched = match ReaderSourceHints::lend(&mut state, native, route) {
        Ok(mut record) => record.load(digest, &mut previous) && previous == current,
        Err(_) => false,
    };
    let closed = state.close(native, route);
    matched && closed.is_ok_and(|outcome| outcome.clean)
}
pub(super) fn record(native: &mut impl Native, route: &Route<'_>, digest: &[u8; 32],
    current: SourceChangeHints,
) {
    if !current.valid() { return; }
    let mut state = ReaderSourceHintsState::new(native, route);
    if let Ok(mut record) = ReaderSourceHints::lend(&mut state, native, route) {
        let mut previous = SourceChangeHints::default();
        if !record.load(digest, &mut previous) || previous != current { record.save(digest, current); }
    }
    let _ = state.close(native, route);
}

impl super::Runtime {
    pub(super) fn receipt_present_image(&mut self, home: &mut crate::app_image::Home<'_>, filename: &core::ffi::CStr) -> bool {
        let Ok(filename) = filename.to_str() else { return false; };
        let mut key = [0; 64];
        let mut hash = crate::reader_hash::PackageHasher::from_live_sdk();
        let Some(key) = crate::reader_book::route_key(filename, &mut hash, &mut key) else { return false; };
        home.with_storage_io(|_, io| receipt_present_on(io, key)).unwrap_or(false)
    }
}

// Same receipt probe for the short panel/source scope; no remount or extra load.
pub(super) fn receipt_present_on(io: &newt_native_io::UiIo, key: &str) -> bool {
        let route = newt_native_storage::RouteState::new(crate::reader_book::CACHE_ROOT, key);
        let route = route.lend(&super::boot::NoCancel, super::millis);
            let mut native = io.port();
            let mut state = crate::sd_revision::SdRevisionState::new(&mut native, &route);
            let mut bytes = 0;
            let found = match crate::sd_revision::SdRevisionStorage::lend(&mut state, &mut native, &route) {
                Ok(mut storage) => newt_phased_reader::imported_revision::RevisionStorage::load(&mut storage, &mut bytes),
                Err(_) => false,
            };
            let closed = state.close(&mut native, &route);
            found && bytes != 0 && closed.is_ok_and(|outcome| outcome.clean)

}
