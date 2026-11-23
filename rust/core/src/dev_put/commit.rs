// SPDX-License-Identifier: MIT
//! DONE's commit and the recovery before each FILE.
//!
//! A FAT rename writes the new directory entry before it removes the old one,
//! so a power loss between the two leaves both names on one cluster chain, and
//! removing either name frees the other's data. No content check can tell that
//! from two equal copies. Nor does a long name's absence prove its entry gone:
//! FatFs removes a name's long-name entries before its short one, so a power
//! loss between them leaves a hidden short alias on the data while `stat` of
//! the long name reports it missing. So DONE syncs a marker naming its target
//! before any rename, new files included, and removes it only after both
//! renames and the backup's removal; a marker the next FILE finds refuses the
//! folder, and recovery deletes nothing.
use super::{write_all, Lease, Refused};
use crate::dev_put::{Entry, Folder};
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
        // A marker this leaves refuses the folder's next FILE: the hand check clears it.
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

    /// Before any write to the folder. A marker that outlived its DONE means a
    /// commit stopped somewhere between its marker and its cleanup, and no
    /// presence check can say where (see the module note), so it refuses every
    /// write until the card is checked by hand. Without one no rename is left
    /// unresolved.
    pub(super) fn recover(&mut self, folder: Folder) -> Result<(), Refused> {
        let old = self.hidden(folder.old(), "backup")?;
        if self.hidden(folder.marker(), "marker")? { return Err(Refused::Recovery("commit", 0)); }
        // FILE then removes a leftover temp; a backup without its marker has no known target.
        if old { return Err(Refused::Recovery("backup", 0)); }
        Ok(())
    }

    fn hidden(&mut self, path: &CStr, step: &'static str) -> Result<bool, Refused> {
        match self.card.entry(path).map_err(|code| Refused::Recovery(step, code))? {
            Entry::Missing => Ok(false),
            Entry::File => Ok(true),
            Entry::Folder | Entry::Other => Err(Refused::Recovery(step, 0)),
        }
    }
}
