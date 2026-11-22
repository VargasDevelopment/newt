pub use newt_core::library_catalog::{LibraryOmission, LibraryOmissions, LibraryStatus};
use crate::{library_name::name_bytes, list, listable};
use alloc::vec::Vec;
use core::cmp::Ordering;
pub type Name = [u8; 96];
pub const TABLE: usize = 128;
pub const WINDOW: usize = 4;
pub const ENTRIES: u32 = 32768;
pub const DEPTH: usize = 6;
pub const PROOFS: u16 = 128;
#[cfg(test)]
pub(crate) mod allocation_tests;
pub(crate) fn before(a: &[u8], b: &[u8]) -> bool {
    fn fold(a: &[u8], b: &[u8]) -> Ordering {
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase))
    }
    let abase = crate::library_base_name(Some(a)).unwrap();
    let bbase = crate::library_base_name(Some(b)).unwrap();
    fold(abase, bbase)
        .then_with(|| fold(name_bytes(a).unwrap(), name_bytes(b).unwrap()))
        .then_with(|| name_bytes(a).unwrap().cmp(name_bytes(b).unwrap()))
        .is_lt()
}
fn kept(a: &[u8], b: &[u8]) -> bool {
    if (a[0] == b':') != (b[0] == b':') {
        a[0] != b':'
    } else {
        before(a, b)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Added {
    Listed,
    Copy,
    Beyond,
    Omitted,
}
const TABLE_BYTES: usize = TABLE * (96 + 4);
/// Borrowed views only: the owner initializes flat bytes directly in its Vec.
pub(crate) struct Table<'a> {
    pub(crate) names: &'a mut [Name],
    bytes: &'a mut [u8],
    pub(crate) count: usize,
}
impl<'a> Table<'a> {
    fn view(storage: &'a mut [u8], count: usize) -> Self {
        assert_eq!(storage.len(), TABLE_BYTES);
        let (names, bytes) = storage.split_at_mut(TABLE * 96);
        let (names, remainder) = names.as_chunks_mut::<96>();
        assert!(remainder.is_empty());
        Self {
            names,
            bytes,
            count,
        }
    }
    pub(crate) fn value(&self, index: usize) -> u32 {
        let bytes = &self.bytes.as_chunks::<4>().0[index];
        u32::from_le_bytes(*bytes)
    }
    pub(crate) fn set_value(&mut self, index: usize, value: u32) {
        self.bytes.as_chunks_mut::<4>().0[index].copy_from_slice(&value.to_le_bytes());
    }
}
struct Window {
    names: [Name; 4],
    first: Option<usize>,
    path: [u8; 192],
}
/// Empty/default holds no names. Each nonempty Vec owns exactly one fallibly
/// reserved byte table OR window. No allocating sort; no retained reader table.
pub struct Catalog {
    table: Vec<u8>,
    table_count: usize,
    window: Vec<Window>,
    count: u32,
    outside: u32,
    status: LibraryStatus,
    finished: bool,
    copies: u8,
    omissions: LibraryOmissions,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            table: Vec::new(),
            table_count: 0,
            window: Vec::new(),
            count: 0,
            outside: 0,
            status: LibraryStatus::Ok,
            finished: false,
            copies: 0,
            omissions: LibraryOmissions::default(),
        }
    }
}
impl Catalog {
    pub fn reset(&mut self, status: LibraryStatus) {
        *self = Self::default();
        self.status = status;
    }
    fn ensure_table(&mut self) -> bool {
        if self.table.is_empty() {
            let Some(table) = crate::zeroed(TABLE_BYTES) else {
                self.reset(LibraryStatus::Unavailable);
                return false;
            };
            self.table = table;
        }
        true
    }
    pub fn status(&self) -> LibraryStatus {
        self.status
    }
    pub fn size(&self) -> usize {
        if self.finished && self.status == LibraryStatus::Ok {
            self.count as usize
        } else {
            0
        }
    }
    pub fn outside(&self) -> u32 {
        self.outside
    }
    pub fn on_card(&self) -> bool {
        !self.window.is_empty()
    }
    pub fn omissions(&self) -> &LibraryOmissions {
        &self.omissions
    }
    pub fn copies(&self) -> u8 {
        self.copies
    }
    pub(crate) fn table(&mut self) -> Table<'_> {
        Table::view(&mut self.table, self.table_count)
    }
    pub fn add(
        &mut self,
        name: &[u8],
        bytes: u32,
        mut same: Option<&mut dyn FnMut(&[u8], &[u8]) -> bool>,
        beyond: bool,
    ) -> Added {
        if self.status != LibraryStatus::Ok {
            return Added::Omitted;
        }
        if self.finished || self.on_card() {
            self.reset(LibraryStatus::Unavailable);
            return Added::Omitted;
        }
        if !listable(name) {
            self.omissions.names = self.omissions.names.saturating_add(1);
            return Added::Omitted;
        }
        if !self.ensure_table() {
            return Added::Omitted;
        }
        let mut table = Table::view(&mut self.table, self.table_count);
        if let Some(same) = same.as_mut() {
            for i in 0..table.count {
                if bytes == 0 || table.value(i) != bytes || !same(&table.names[i], name) {
                    continue;
                }
                self.copies = self.copies.saturating_add(1);
                if kept(name, &table.names[i]) {
                    copy_name(&mut table.names[i], name);
                }
                return Added::Copy;
            }
        }
        let mut slot = table.count;
        if slot == TABLE {
            if beyond {
                return Added::Beyond;
            }
            self.omissions.full = self.omissions.full.saturating_add(1);
            slot = 0;
            for i in 1..table.count {
                if kept(&table.names[slot], &table.names[i]) {
                    slot = i;
                }
            }
            if !kept(name, &table.names[slot]) {
                return Added::Omitted;
            }
        } else {
            table.count += 1;
            self.table_count = table.count;
        }
        copy_name(&mut table.names[slot], name);
        table.set_value(slot, bytes);
        Added::Listed
    }
    pub fn omit(&mut self, what: LibraryOmission) {
        if self.status != LibraryStatus::Ok || self.finished {
            return;
        }
        match what {
            LibraryOmission::Name => self.omissions.names = self.omissions.names.saturating_add(1),
            LibraryOmission::Folder => {
                self.omissions.folders = self.omissions.folders.saturating_add(1)
            }
            LibraryOmission::Stopped => self.omissions.stopped = true,
        }
    }
    pub fn finish(&mut self) -> LibraryStatus {
        if self.status != LibraryStatus::Ok || self.finished || !self.ensure_table() {
            return self.status;
        }
        let t = self.table();
        sort(&mut t.names[..t.count]);
        let count = t.count as u32;
        let outside = t.names[..t.count].iter().filter(|n| n[0] == b':').count() as u32;
        self.count = count;
        self.outside = outside;
        self.finished = true;
        self.status
    }
    pub fn use_list(&mut self, path: &[u8], count: u32, outside: u32) -> LibraryStatus {
        if self.status != LibraryStatus::Ok {
            return self.status;
        }
        self.table = Vec::new();
        self.table_count = 0;
        self.window = Vec::new();
        let len = path.iter().position(|&b| b == 0);
        if len.is_none_or(|n| n >= 192) || self.window.try_reserve_exact(1).is_err() {
            self.reset(LibraryStatus::Unavailable);
            return self.status;
        }
        let mut w = Window {
            names: [[0; 96]; 4],
            first: None,
            path: [0; 192],
        };
        let n = len.unwrap();
        w.path[..n].copy_from_slice(&path[..n]);
        self.window.push(w);
        self.count = count;
        self.outside = outside;
        self.finished = true;
        self.status
    }
    /// Return borrows self, so shelf replacement cannot invalidate a live loan.
    pub fn filename<'a>(
        &'a mut self,
        files: &mut dyn list::Files,
        index: usize,
    ) -> Option<&'a [u8]> {
        if index >= self.size() {
            return None;
        }
        if !self.on_card() {
            let (names, _) = self.table.split_at(TABLE * 96);
            return Some(c_name(&names.as_chunks::<96>().0[index]));
        }
        let first = index / 4 * 4;
        let w = &mut self.window[0];
        if w.first != Some(first) {
            w.first = None;
            for name in &mut w.names {
                name[0] = 0;
            }
            let fd = files.open(&w.path, false);
            if fd >= 0 {
                let mut damaged = false;
                for i in 0..4.min(self.count as usize - first) {
                    if !list::read_record(
                        files,
                        fd,
                        list::at(list::HEADER, (first + i) as u32),
                        &mut w.names[i],
                    ) {
                        damaged = true;
                        break;
                    }
                }
                files.close(fd);
                if damaged {
                    for name in &mut w.names {
                        name[0] = 0;
                    }
                    files.remove(&w.path);
                } else {
                    w.first = Some(first);
                }
            }
        }
        let name = &w.names[index - first];
        if name[0] == 0 {
            None
        } else {
            Some(c_name(name))
        }
    }
    pub fn readable(&mut self, files: &mut dyn list::Files, index: usize) -> bool {
        !self.on_card()
            || self.size() == 0
            || self.filename(files, index.min(self.size() - 1)).is_some()
    }
    /// Lend the existing visible name window while its catalog cannot change.
    /// On-card callers use shelf-aligned first indices to receive four names.
    pub fn with_shelf_names<R>(&mut self, files: &mut dyn list::Files, first: usize,
        work: impl FnOnce(&[Name]) -> R,
    ) -> R {
        let size = self.size();
        if first >= size { return work(&[]); }
        if self.on_card() {
            if self.filename(files, first).is_none() { return work(&[]); }
            let slot = first % WINDOW;
            let count = (size - first).min(WINDOW - slot);
            work(&self.window[0].names[slot..slot + count])
        } else {
            let (names, _) = self.table.split_at(TABLE * 96);
            let names = names.as_chunks::<96>().0;
            work(&names[first..first + (size - first).min(WINDOW)])
        }
    }
    pub fn path(&mut self, files: &mut dyn list::Files, index: usize, out: &mut [u8]) -> bool {
        crate::library_book_path(
            Some(b"/Newt/Books\0"),
            self.filename(files, index),
            Some(out),
        )
    }
    pub fn notice(&self) -> Option<&'static str> {
        if self.status != LibraryStatus::Ok {
            None
        } else if self.omissions.stopped {
            Some("The card has too many files to list all.")
        } else if self.omissions.full != 0 {
            Some("Not every book is shown; the card may be full.")
        } else if self.omissions.names != 0 {
            Some("Some books have names Newt can't show.")
        } else if self.omissions.folders != 0 {
            Some("Some folders are too deep or unreadable.")
        } else {
            None
        }
    }
}
pub(crate) fn c_name(name: &[u8]) -> &[u8] {
    &name[..name_bytes(name).unwrap().len() + 1]
}
fn copy_name(out: &mut Name, name: &[u8]) {
    let n = name_bytes(name).unwrap().len();
    out[..n + 1].copy_from_slice(&name[..n + 1]);
}
pub(crate) fn sort(names: &mut [Name]) {
    names.sort_unstable_by(|a, b| {
        if before(a, b) {
            Ordering::Less
        } else if before(b, a) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
}
