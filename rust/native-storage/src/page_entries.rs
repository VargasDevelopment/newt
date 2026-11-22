//! Frozen page_entry_directory.h's scoped visitor; target keeps FatFs metadata.
#![forbid(unsafe_code)]
use crate::Native;

pub struct PageEntry<'name> {
    pub name: Option<&'name [u8]>,
    pub bytes: u64,
    pub time: i64,
    pub regular: bool,
}

/// The directory and its name buffer stay inside the walk. A short reborrow
/// permits the frozen names pass to delete a selected victim as it is met.
/// There is no default: the target must use its metadata-carrying FatFs walk.
pub trait PageEntries: Native {
    fn walk_entries(&mut self, folder: &core::ffi::CStr, sized: bool,
        visit: &mut dyn FnMut(&mut Self, PageEntry<'_>) -> bool,
    ) -> bool;
}

/// POSIX host branch only; target VFS per-name stats would cost O(n²).
#[cfg(not(target_os = "none"))]
pub fn walk_directory<N: Native + crate::Directory>(native: &mut N, folder: &core::ffi::CStr,
    sized: bool, visit: &mut dyn FnMut(&mut N, PageEntry<'_>) -> bool,
) -> bool {
    let Ok(folder) = folder.to_str() else { return false; };
    let Ok(mut listing) = native.open_dir(folder) else { return false; };
    let mut name = [0; 256];
    let mut path = [0; 400];
    let good = loop {
        let length = match native.read_dir(&mut listing, &mut name) {
            Ok(length) => length, Err(_) => break false,
        };
        let Some(length) = length else {
            break visit(native, PageEntry { name: None, bytes: 0, time: 0, regular: false });
        };
        if length > name.len() { break false; }
        let name = &name[..length];
        if name == b"." || name == b".." { continue; }
        let mut entry = PageEntry { name: Some(name), bytes: 0, time: 0, regular: false };
        if sized {
            let Some(bytes) = folder.len().checked_add(1).and_then(|n| n.checked_add(name.len())) else { break false; };
            if bytes >= path.len() { break false; }
            path[..folder.len()].copy_from_slice(folder.as_bytes());
            path[folder.len()] = b'/';
            path[folder.len()+1..bytes].copy_from_slice(name);
            let Ok(path) = core::str::from_utf8(&path[..bytes]) else { break false; };
            let Ok(stat) = native.stat(path, true) else { break false; };
            entry.bytes = stat.size as u64;
            entry.time = stat.seconds;
            entry.regular = stat.kind == crate::Kind::Regular && stat.size >= 0;
        }
        if !visit(native, entry) { break false; }
    };
    native.close_dir(listing).is_ok() && good
}
