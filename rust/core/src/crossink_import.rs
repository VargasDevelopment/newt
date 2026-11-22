// SPDX-License-Identifier: MIT
//! Read-only CrossInk import policy from firmware/crossink_import.h at 31423dbf.
//! Native adapters own handles and SHA contexts; only Newt's store receives writes.
#![forbid(unsafe_code)]
use core::ffi::CStr;

use crate::crossink_stats as ci;
use crate::reading_stats_once::Sha256;
use crate::reading_stats_store::{
    CROSS_INK_NAMESPACE, StatsBackend, StatsEffects, StatsLoad, StatsSave, StatsStore,
    cross_ink_source_namespace, load, save, valid_cross_ink_global,
};
use crate::sleep_stats::StatsStreak;

pub use crate::book_stats_import::cross_ink_import_frozen;
mod candidates;
mod io;
use candidates::{read_global, read_source};
pub use io::{BookHasher, Entry, EntryKind, Files, HashFactory, IoError};
use io::{Found, Path, epub_file, epub_source_name, hash_book};

const PATH_BYTES: usize = 256;
const WALK_DEPTH: usize = 6;
const BOOK_MAX_BYTES: u64 = 64 * 1024 * 1024;
const IMPORT_NAMESPACE: &str = "newt_import";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportResult {
    Done,
    Nothing,
    Cancelled,
    Unavailable,
    Unconfirmed,
    Unsupported,
}
impl ImportResult {
    pub fn name(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Nothing => "nothing",
            Self::Cancelled => "cancelled",
            Self::Unavailable => "unavailable",
            Self::Unconfirmed => "unconfirmed",
            Self::Unsupported => "unsupported",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportReport {
    pub result: ImportResult,
    pub sources: u16,
    pub bound: u16,
}
impl Default for ImportReport {
    fn default() -> Self {
        Self {
            result: ImportResult::Done,
            sources: 0,
            bound: 0,
        }
    }
}
fn loaded_outcome(loaded: StatsLoad) -> ImportResult {
    if loaded == StatsLoad::Unavailable {
        ImportResult::Unavailable
    } else {
        ImportResult::Done
    }
}
fn saved_outcome(saved: StatsSave) -> ImportResult {
    match saved {
        StatsSave::Saved => ImportResult::Done,
        StatsSave::Unconfirmed => ImportResult::Unconfirmed,
        StatsSave::Unsupported => ImportResult::Unsupported,
        StatsSave::Corrupt | StatsSave::Unavailable => ImportResult::Unavailable,
    }
}
fn freeze<B: StatsBackend, const N: usize>(
    effects: &StatsEffects,
    store: &mut B,
    name: &str,
    magic: u32,
    valid: impl Fn(&[u8]) -> bool,
    encoded: [u8; N],
) -> ImportResult {
    let mut saved = save(effects, store, name, magic, &valid, encoded);
    if saved == StatsSave::Corrupt && store.set_aside(name) {
        saved = save(effects, store, name, magic, valid, encoded);
    }
    saved_outcome(saved)
}
#[inline(never)]
fn clear_streak_cache<B: StatsBackend>(effects: &StatsEffects, store: &mut B) -> ImportResult {
    let mut stats = StatsStore::new(store);
    let mut cache = StatsStreak::default();
    let loaded = stats.load_streak(&mut cache);
    if loaded == StatsLoad::Unsupported {
        return ImportResult::Unsupported;
    }
    if loaded != StatsLoad::Loaded {
        return loaded_outcome(loaded);
    }
    if !cache.stamped() {
        return ImportResult::Done;
    }
    saved_outcome(stats.save_streak(effects, &StatsStreak::default()))
}
struct Run<'a, B, F, H> {
    store: &'a mut B,
    files: &'a mut F,
    hash: &'a mut H,
    root: &'a CStr,
    cancelled: &'a mut dyn FnMut() -> bool,
    sha256: Sha256<'a>,
    report: &'a mut ImportReport,
}
impl<B: StatsBackend, F: Files, H: HashFactory> Run<'_, B, F, H> {
    #[inline(never)]
    fn import_global(&mut self, found: &mut bool) -> ImportResult {
        let loaded = load::<_, { ci::GLOBAL_RECORD_BYTES }>(
            self.store,
            CROSS_INK_NAMESPACE,
            ci::GLOBAL_MAGIC,
            valid_cross_ink_global,
            |_| {},
        );
        *found = loaded != StatsLoad::Absent && loaded != StatsLoad::Corrupt;
        if *found {
            return loaded_outcome(loaded);
        }
        let mut copy = ci::CrossInkGlobalCopy::default();
        match read_global(self.files, self.root, &mut copy) {
            Found::Failed => return ImportResult::Unavailable,
            Found::Missing => return ImportResult::Done,
            Found::Read => *found = true,
        }
        let Some(effects) = StatsStore::<B>::begin_effects(Some(self.cancelled)) else {
            return ImportResult::Cancelled;
        };
        let cleared = clear_streak_cache(&effects, self.store);
        if cleared != ImportResult::Done {
            return cleared;
        }
        let Some(encoded) = ci::encode_cross_ink_global(&copy) else {
            return ImportResult::Unavailable;
        };
        freeze(
            &effects,
            self.store,
            CROSS_INK_NAMESPACE,
            ci::GLOBAL_MAGIC,
            valid_cross_ink_global,
            encoded,
        )
    }
    #[inline(never)]
    fn import_source(
        &mut self,
        name: &[u8],
        book: Option<&CStr>,
        frozen: &mut bool,
    ) -> ImportResult {
        let ns = cross_ink_source_namespace(name, self.sha256);
        let valid = |bytes: &[u8]| {
            ci::decode_cross_ink_source(bytes).is_some_and(|source| {
                source.name.get(..name.len()) == Some(name)
                    && source.name.get(name.len()) == Some(&0)
            })
        };
        let loaded = load::<_, { ci::SOURCE_RECORD_BYTES }>(
            self.store,
            ns.as_str(),
            ci::SOURCE_MAGIC,
            valid,
            |_| {},
        );
        *frozen = loaded == StatsLoad::Loaded || loaded == StatsLoad::Unsupported;
        if loaded != StatsLoad::Absent && loaded != StatsLoad::Corrupt {
            return loaded_outcome(loaded);
        }
        let mut source = ci::CrossInkSource::default();
        source.name[..name.len()].copy_from_slice(name);
        match read_source(self.files, self.root, name, &mut source.copy) {
            Found::Failed => return ImportResult::Unavailable,
            Found::Missing => return ImportResult::Done,
            Found::Read => {}
        }
        if let Some(path) = book {
            let mut stopped = false;
            let hashed = hash_book(
                self.files,
                self.hash,
                path,
                &mut source.digest,
                self.cancelled,
                &mut stopped,
            );
            if stopped {
                return ImportResult::Cancelled;
            }
            if hashed == Found::Failed {
                return ImportResult::Unavailable;
            }
            if hashed == Found::Read {
                source.binding = ci::CrossInkBinding::Unverified;
            }
        }
        let Some(effects) = StatsStore::<B>::begin_effects(Some(self.cancelled)) else {
            return ImportResult::Cancelled;
        };
        let Some(encoded) = ci::encode_cross_ink_source(&source) else {
            return ImportResult::Unavailable;
        };
        let result = freeze(
            &effects,
            self.store,
            ns.as_str(),
            ci::SOURCE_MAGIC,
            valid,
            encoded,
        );
        *frozen = result == ImportResult::Done;
        if *frozen && source.binding != ci::CrossInkBinding::None {
            self.report.bound = self.report.bound.wrapping_add(1);
        }
        result
    }
    fn bind_book(&mut self, path: &Path) -> ImportResult {
        let name = epub_source_name(&path.bytes[self.root.to_bytes().len()..path.len]);
        let folder = Path::new(&[self.root.to_bytes(), b"/.crosspoint/", name.as_bytes()]);
        let (Ok(folder), Ok(book)) = (folder.c_str(), path.c_str()) else {
            return ImportResult::Unavailable;
        };
        match self.files.is_directory(folder) {
            Ok(true) => self.import_source(name.as_bytes(), Some(book), &mut false),
            Ok(false) | Err(IoError::Missing) => ImportResult::Done,
            Err(_) => ImportResult::Unavailable,
        }
    }
    #[inline(never)]
    fn bind_books(&mut self) -> ImportResult {
        let mut path = Path::new(&[self.root.to_bytes()]);
        let Ok(root_path) = path.c_str() else {
            return ImportResult::Unavailable;
        };
        let first = match self.files.open_directory(root_path) {
            Ok(directory) => directory,
            Err(IoError::Missing) => return ImportResult::Done,
            Err(_) => return ImportResult::Unavailable,
        };
        let mut levels: [Option<F::Directory>; WALK_DEPTH + 1] = core::array::from_fn(|_| None);
        let mut lengths = [0; WALK_DEPTH + 1];
        levels[0] = Some(first);
        lengths[0] = path.len;
        let mut depth = 0;
        let mut result = ImportResult::Done;
        loop {
            let length = lengths[depth];
            path.truncate(length);
            if (self.cancelled)() {
                result = ImportResult::Cancelled;
                break;
            }
            let next = match levels[depth].as_mut() {
                Some(directory) => self.files.next(directory),
                None => Err(IoError::Failed),
            };
            let entry = match next {
                Ok(Some(entry)) => entry,
                Ok(None) => {
                    if let Some(directory) = levels[depth].take() {
                        let _ = self.files.close_directory(directory);
                    }
                    if depth == 0 {
                        return result;
                    }
                    depth -= 1;
                    continue;
                }
                Err(_) => {
                    result = ImportResult::Unavailable;
                    break;
                }
            };
            let name = entry.name.to_bytes();
            let newt = &path.bytes[self.root.to_bytes().len()..path.len] == b"/Newt";
            if name.first() == Some(&b'.') || (newt && name != b"Books") {
                continue;
            }
            if length + 1 + name.len() >= PATH_BYTES {
                continue;
            }
            path.append(b"/");
            path.append(name);
            let Ok(current_path) = path.c_str() else {
                result = ImportResult::Unavailable;
                break;
            };
            let folder = match entry.kind {
                EntryKind::Directory => true,
                EntryKind::Unknown => self.files.is_directory(current_path).unwrap_or(false),
                EntryKind::Other => false,
            };
            let epub = epub_file(name);
            if folder && depth < WALK_DEPTH {
                match self.files.open_directory(current_path) {
                    Ok(child) => {
                        depth += 1;
                        levels[depth] = Some(child);
                        lengths[depth] = path.len;
                    }
                    Err(IoError::Missing) => {}
                    Err(_) => {
                        result = ImportResult::Unavailable;
                        break;
                    }
                }
            } else if !folder && epub {
                result = self.bind_book(&path);
                if result != ImportResult::Done {
                    break;
                }
            }
        }
        for directory in levels.iter_mut().take(depth + 1).filter_map(Option::take) {
            let _ = self.files.close_directory(directory);
        }
        result
    }
    #[inline(never)]
    fn import_sources(&mut self, freeze_sources: bool, sources: &mut u32) -> ImportResult {
        let path = Path::new(&[self.root.to_bytes(), b"/.crosspoint"]);
        let Ok(current_path) = path.c_str() else {
            return ImportResult::Unavailable;
        };
        let mut directory = match self.files.open_directory(current_path) {
            Ok(directory) => directory,
            Err(IoError::Missing) => return ImportResult::Done,
            Err(_) => return ImportResult::Unavailable,
        };
        let mut result = ImportResult::Done;
        loop {
            if (self.cancelled)() {
                result = ImportResult::Cancelled;
                break;
            }
            let entry = match self.files.next(&mut directory) {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(_) => {
                    result = ImportResult::Unavailable;
                    break;
                }
            };
            let name = entry.name.to_bytes();
            if !ci::cross_ink_source_name(name) {
                continue;
            }
            let mut frozen = false;
            if freeze_sources {
                result = self.import_source(name, None, &mut frozen);
            } else {
                let mut copy = ci::CrossInkCopy::default();
                let read = read_source(self.files, self.root, name, &mut copy);
                frozen = read == Found::Read;
                if read == Found::Failed {
                    result = ImportResult::Unavailable;
                }
            }
            if frozen {
                *sources = sources.wrapping_add(1);
            }
            if result != ImportResult::Done || (!freeze_sources && *sources != 0) {
                break;
            }
        }
        let _ = self.files.close_directory(directory);
        result
    }
}
/// The global copy is rechecked even when the marker already exists. Every
/// completed source is immutable; the final marker is the last commit.
#[inline(never)]
pub fn import_cross_ink<B: StatsBackend, F: Files, H: HashFactory>(
    store: &mut B,
    files: &mut F,
    hash: &mut H,
    card_root: &CStr,
    cancelled: &mut dyn FnMut() -> bool,
    sha256: Sha256<'_>,
) -> ImportReport {
    let mut report = ImportReport::default();
    if card_root.to_bytes().len() >= PATH_BYTES {
        report.result = ImportResult::Unavailable;
        return report;
    }
    let mut run = Run {
        store,
        files,
        hash,
        root: card_root,
        cancelled,
        sha256,
        report: &mut report,
    };
    let marker = load::<_, { ci::IMPORT_RECORD_BYTES }>(
        run.store,
        IMPORT_NAMESPACE,
        ci::IMPORT_MAGIC,
        |bytes| ci::decode_import(bytes).is_some(),
        |_| {},
    );
    if marker == StatsLoad::Unavailable || marker == StatsLoad::Unsupported {
        run.report.result = if marker == StatsLoad::Unavailable {
            ImportResult::Unavailable
        } else {
            ImportResult::Unsupported
        };
        return report;
    }
    let mut global = false;
    run.report.result = run.import_global(&mut global);
    if run.report.result != ImportResult::Done || marker == StatsLoad::Loaded {
        return report;
    }
    let mut sources = 0;
    if !global {
        run.report.result = run.import_sources(false, &mut sources);
        if run.report.result != ImportResult::Done {
            return report;
        }
        if sources == 0 {
            run.report.result = ImportResult::Nothing;
            return report;
        }
        sources = 0;
    }
    run.report.result = run.bind_books();
    if run.report.result == ImportResult::Done {
        run.report.result = run.import_sources(true, &mut sources);
    }
    if run.report.result != ImportResult::Done {
        return report;
    }
    let Some(effects) = StatsStore::<B>::begin_effects(Some(run.cancelled)) else {
        run.report.result = ImportResult::Cancelled;
        return report;
    };
    let encoded = ci::encode_import(&ci::ImportRecord {
        sources,
        ..ci::ImportRecord::default()
    });
    run.report.result = freeze(
        &effects,
        run.store,
        IMPORT_NAMESPACE,
        ci::IMPORT_MAGIC,
        |bytes| ci::decode_import(bytes).is_some(),
        encoded,
    );
    run.report.sources = sources.min(u16::MAX.into()) as u16;
    report
}

#[cfg(test)]
#[path = "crossink_import_tests.rs"]
mod tests;
