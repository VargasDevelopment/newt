// SPDX-License-Identifier: MIT
//! DONE's commit and the recovery before each FILE.
//!
//! A FAT rename writes the new directory entry before it removes the old one,
//! so a power loss between the two leaves both names on one cluster chain, and
//! removing either name frees the other's data. No content check can tell that
//! from two equal copies. So DONE syncs a marker naming its target before any
//! rename, new files included; while the marker stands, no rename in its folder
//! is known to be complete, and recovery deletes nothing that might share data.
use super::{path, target_path, write_all, Lease, Refused};
use crate::dev_put::{name_allowed, Entry, Folder, NAME_BYTES, PATH_BYTES};
use core::ffi::CStr;

impl Lease<'_> {
    /// Syncs the marker naming DONE's target before its first rename.
    pub(super) fn mark(&mut self, folder: Folder, name: &[u8]) -> Result<(), Refused> {
        // FILE resolved any earlier commit; never overwrite the marker of one.
        if self.card.entry(folder.old()).map_err(|code| Refused::Card("backup", code))? != Entry::Missing {
            return Err(Refused::Recovery("backup", 0));
        }
        let fd = self.card.create(folder.marker()).map_err(|code| Refused::Card("mark", code))?;
        let synced = write_all(self.card, fd, name).and_then(|()| self.card.sync(fd));
        let closed = self.card.close(fd);
        if let Err(code) = synced.and(closed) {
            let _ = self.card.remove(folder.marker());
            return Err(Refused::Card("mark", code));
        }
        Ok(())
    }

    /// Moves an old target aside, renames the temp into place and drops the
    /// backup. Any refused rename leaves the marker unless the folder reads as
    /// it did before DONE.
    pub(super) fn commit(&mut self, folder: Folder, target: &CStr, replacing: bool) -> Result<(), Refused> {
        if replacing {
            if let Err(code) = self.card.rename(target, folder.old()) {
                return Err(self.undone(folder, target, replacing, "replace", code));
            }
        }
        if let Err(code) = self.card.rename(folder.temp(), target) {
            if replacing && self.card.rename(folder.old(), target).is_err() { return Err(Refused::Recovery("rename", code)); }
            return Err(self.undone(folder, target, replacing, "rename", code));
        }
        // Both renames completed, so the backup shares no data with the target.
        // FatFs's f_unlink removes the entry before it frees the chain, and its
        // one-sector window writes the entry's sector before it loads a FAT
        // sector: a power loss inside the unlink only leaks clusters.
        if replacing { self.card.remove(folder.old()).map_err(|code| Refused::Recovery("backup", code))?; }
        // A marker left here names a target alone in its folder; the next FILE drops it.
        let _ = self.card.remove(folder.marker());
        Ok(())
    }

    /// A refused rename is a plain card refusal only when the target, backup
    /// and temp read exactly as before DONE (a rollback complete); a rename can
    /// fail in its final sync after its entries changed.
    fn undone(&mut self, folder: Folder, target: &CStr, replacing: bool, step: &'static str, code: i32) -> Refused {
        let before = if replacing { Entry::File } else { Entry::Missing };
        let unchanged = self.card.entry(target) == Ok(before) && self.card.entry(folder.old()) == Ok(Entry::Missing)
            && self.card.entry(folder.temp()) == Ok(Entry::File);
        if !unchanged { return Refused::Recovery(step, code); }
        if let Err(code) = self.card.remove(folder.marker()) { return Refused::Recovery("marker", code); }
        // Without the marker, a temp left by a failed remove is an abandoned transfer's.
        let _ = self.card.remove(folder.temp());
        Refused::Card(step, code)
    }

    /// Before any write to the folder, resolves what an interrupted commit
    /// left, but only where one name alone is present.
    pub(super) fn recover(&mut self, folder: Folder) -> Result<(), Refused> {
        let old = self.hidden(folder.old(), "backup")?;
        if !self.hidden(folder.marker(), "marker")? {
            // No commit began: FILE removes the temp; a backup without its marker has no known target.
            return if old { Err(Refused::Recovery("backup", 0)) } else { Ok(()) };
        }
        let target = self.marked(folder)?;
        let target = path(&target);
        let present = match self.card.entry(target).map_err(|code| Refused::Recovery("target", code))? {
            Entry::Missing => false,
            Entry::File => true,
            Entry::Folder | Entry::Other => return Err(Refused::Recovery("target", 0)),
        };
        let temp = self.hidden(folder.temp(), "temp")?;
        match (present, old, temp) {
            (false, true, false) => self.card.rename(folder.old(), target).map_err(|code| Refused::Recovery("restore", code))?,
            (false, false, true) => self.card.remove(folder.temp()).map_err(|code| Refused::Recovery("temp", code))?,
            (true, false, false) | (false, false, false) => {}
            // Two names may share one file: nothing here is deleted.
            _ => return Err(Refused::Recovery("commit", 0)),
        }
        self.card.remove(folder.marker()).map_err(|code| Refused::Recovery("marker", code))
    }

    fn hidden(&mut self, path: &CStr, step: &'static str) -> Result<bool, Refused> {
        match self.card.entry(path).map_err(|code| Refused::Recovery(step, code))? {
            Entry::Missing => Ok(false),
            Entry::File => Ok(true),
            Entry::Folder | Entry::Other => Err(Refused::Recovery(step, 0)),
        }
    }

    /// The commit's target from its marker, which must hold one allowed name.
    fn marked(&mut self, folder: Folder) -> Result<[u8; PATH_BYTES], Refused> {
        let fd = self.card.open_read(folder.marker()).map_err(|code| Refused::Recovery("marker", code))?;
        let mut name = [0; NAME_BYTES + 1];
        let mut length = 0;
        let read = loop {
            match self.card.read(fd, &mut name[length..]) {
                Ok(0) => break Ok(()),
                Ok(got) if got <= name.len() - length => {
                    length += got;
                    if length == name.len() { break Ok(()); }
                }
                Ok(_) => break Err(0),
                Err(code) => break Err(code),
            }
        };
        let closed = self.card.close(fd);
        read.and(closed).map_err(|code| Refused::Recovery("marker", code))?;
        let name = &name[..length];
        if !name_allowed(name) { return Err(Refused::Recovery("marker", 0)); }
        Ok(target_path(folder, name))
    }
}
