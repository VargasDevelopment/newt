// SPDX-License-Identifier: MIT
//! Frozen DEV cache purge; derived Cache only, bounded to four directory loans.
use super::Runtime;
use crate::app_image::{Application, Home};
use core::fmt::Write;
use newt_core::app::Screen;
use newt_native_storage::{Directory, Kind, Native};
use newt_native_io::{OsListing, OsNative};

#[derive(Default)]
struct Totals { files: u32, directories: u32, failures: u32, bytes: u32 }
struct Level<'ui> { listing: Option<OsListing<'ui>>, length: usize }

impl Runtime {
    pub(super) fn purge_cache_image(&mut self, home: &mut Home<'_>, ui: &mut Application<'_>) -> bool {
        if ui.app.screen != Screen::Home || self.opened() {
            let _ = writeln!(home.bus.log, "Newt dev purge refused: Home with the reader closed only");
            return false;
        }
        if self.close_runtime(home).is_err() { crate::app_power::stop(); }
        let mut totals = Totals::default();
        let result = home.with_storage_io(|_, io| {
            let mut path = [0; 288];
            let root = crate::reader_book::CACHE_ROOT.as_bytes();
            path[..root.len()].copy_from_slice(root);
            purge_below(&mut io.port(), &mut path, root.len(), &mut totals);
        });
        if result.is_err() { totals.failures = totals.failures.wrapping_add(1); }
        let _ = writeln!(home.bus.log,
            "Newt dev purge: root={} files={} bytes={} directories={} failures={} kept=books,state,covers,settings",
            crate::reader_book::CACHE_ROOT, totals.files, totals.bytes, totals.directories, totals.failures);
        true
    }
}

fn path_text(path: &[u8], length: usize) -> Option<&str> { core::str::from_utf8(&path[..length]).ok() }

fn purge_below(native: &mut OsNative<'_>, path: &mut [u8; 288], length: usize, totals: &mut Totals) {
    let Some(root) = path_text(path, length) else { totals.failures += 1; return; };
    let Ok(listing) = native.open_dir(root) else { totals.failures += 1; return; };
    let mut levels: [Level<'_>; 4] = core::array::from_fn(|_| Level { listing: None, length: 0 });
    levels[0] = Level { listing: Some(listing), length };
    let mut depth = 0;
    loop {
        let level = &mut levels[depth];
        let mut name = [0; 256];
        let entry = match level.listing.as_mut() {
            Some(listing) => native.read_dir(listing, &mut name),
            None => break,
        };
        let count = match entry {
            Ok(Some(count)) => count,
            Ok(None) | Err(_) => {
                if let Some(listing) = level.listing.take() { let _ = native.close_dir(listing); }
                if depth == 0 { break; }
                let removed = path_text(path, level.length).is_some_and(|p| native.rmdir(p).is_ok());
                if removed { totals.directories = totals.directories.wrapping_add(1); }
                else { totals.failures = totals.failures.wrapping_add(1); }
                depth -= 1;
                path[levels[depth].length] = 0;
                continue;
            }
        };
        if count <= name.len() && matches!(&name[..count], b"." | b"..") { continue; }
        let length = level.length;
        let end = length.saturating_add(1).saturating_add(count);
        if count > name.len() || count == 0 || end >= path.len() {
            totals.failures = totals.failures.wrapping_add(1); continue;
        }
        path[length] = b'/';
        path[length + 1..end].copy_from_slice(&name[..count]);
        path[end] = 0;
        let Some(full) = path_text(path, end) else { totals.failures += 1; continue; };
        match native.stat(full, false) {
            Err(_) => totals.failures = totals.failures.wrapping_add(1),
            Ok(info) if info.kind == Kind::Directory => {
                if depth + 1 < levels.len() {
                    if let Ok(listing) = native.open_dir(full) {
                        depth += 1;
                        levels[depth] = Level { listing: Some(listing), length: end };
                        continue;
                    }
                }
                totals.failures = totals.failures.wrapping_add(1);
                if native.rmdir(full).is_ok() { totals.directories = totals.directories.wrapping_add(1); }
                else { totals.failures = totals.failures.wrapping_add(1); }
            }
            Ok(info) => {
                if native.unlink(full).is_ok() {
                    totals.files = totals.files.wrapping_add(1);
                    totals.bytes = totals.bytes.wrapping_add(info.size as u32);
                } else { totals.failures = totals.failures.wrapping_add(1); }
            }
        }
        path[length] = 0;
    }
}
