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
//! card, and recovery deletes nothing.
use super::{write_all, Lease, Refused};
use crate::dev_put::{Entry, TEMP, OLD, MARKER, PATH_BYTES};
use core::ffi::CStr;

impl Lease<'_> {
    /// Syncs the marker naming DONE's target before its first rename.
    pub(super) fn mark(&mut self, name: &[u8]) -> Result<(), Refused> {
        // FILE resolved any earlier commit; never overwrite the marker of one.
        if self.card.entry(OLD).map_err(|code| Refused::Card("backup", code))? != Entry::Missing {
            return Err(Refused::Recovery("backup", 0));
        }
        let fd = self.card.create(MARKER).map_err(|code| Refused::Card("mark", code))?;
        let synced = write_all(self.card, fd, name).and_then(|()| self.card.sync(fd));
        let closed = self.card.close(fd);
        if let Err(code) = synced.and(closed) {
            let _ = self.card.remove(MARKER);
            return Err(Refused::Card("mark", code));
        }
        Ok(())
    }

    /// Moves an old target aside, renames the temp into place and drops the
    /// backup. Any refused rename leaves the marker unless the folder reads as
    /// it did before DONE.
    pub(super) fn commit(&mut self, target: &CStr, replacing: bool) -> Result<(), Refused> {
        if replacing {
            if let Err(code) = self.card.rename(target, OLD) {
                return Err(self.undone(target, replacing, "replace", code));
            }
        }
        if let Err(code) = self.card.rename(TEMP, target) {
            if replacing && self.card.rename(OLD, target).is_err() { return Err(Refused::Recovery("rename", code)); }
            return Err(self.undone(target, replacing, "rename", code));
        }
        // Both renames completed, so the backup shares no data with the target.
        // FatFs's f_unlink removes the entry before it frees the chain, and its
        // one-sector window writes the entry's sector before it loads a FAT
        // sector: a power loss inside the unlink only leaks clusters.
        if replacing { self.card.remove(OLD).map_err(|code| Refused::Recovery("backup", code))?; }
        // A marker this leaves refuses the next FILE: the hand check clears it.
        let _ = self.card.remove(MARKER);
        Ok(())
    }

    /// A refused rename is a plain card refusal only when the target, backup
    /// and temp read exactly as before DONE (a rollback complete); a rename can
    /// fail in its final sync after its entries changed.
    fn undone(&mut self, target: &CStr, replacing: bool, step: &'static str, code: i32) -> Refused {
        let before = if replacing { Entry::File } else { Entry::Missing };
        let unchanged = self.card.entry(target) == Ok(before) && self.card.entry(OLD) == Ok(Entry::Missing)
            && self.card.entry(TEMP) == Ok(Entry::File);
        if !unchanged { return Refused::Recovery(step, code); }
        if let Err(code) = self.card.remove(MARKER) { return Refused::Recovery("marker", code); }
        // Without the marker, a temp left by a failed remove is an abandoned transfer's.
        let _ = self.card.remove(TEMP);
        Refused::Card(step, code)
    }

    /// Before any write to the card. A marker that outlived its DONE means a
    /// commit stopped somewhere between its marker and its cleanup, and no
    /// presence check can say where (see the module note), so it refuses every
    /// write until the card is checked by hand. Without one no rename is left
    /// unresolved.
    pub(super) fn recover(&mut self) -> Result<(), Refused> {
        let old = self.recovery_file(OLD, "backup")?;
        if self.recovery_file(MARKER, "marker")? { return Err(Refused::Recovery("commit", 0)); }
        // FILE then removes an abandoned temp; a backup alone has no known target.
        if old { return Err(Refused::Recovery("backup", 0)); }
        Ok(())
    }

    /// Parents were checked without following links before these old paths
    /// are probed. New transfers have a single recovery namespace at the root.
    pub(super) fn recover_legacy(&mut self, target: &CStr) -> Result<(), Refused> {
        let parent_end = target.to_bytes().iter().rposition(|&b| b == b'/').ok_or(Refused::Name)? + 1;
        let mut legacy = [0; PATH_BYTES];
        legacy[..parent_end].copy_from_slice(&target.to_bytes()[..parent_end]);
        for name in [b".newt-old-name".as_slice(), b".newt-old"] {
            if parent_end + name.len() >= PATH_BYTES { continue; }
            legacy[parent_end..].fill(0);
            legacy[parent_end..parent_end + name.len()].copy_from_slice(name);
            let path = CStr::from_bytes_until_nul(&legacy).map_err(|_| Refused::Name)?;
            if self.recovery_file(path, "legacy")? { return Err(Refused::Recovery("legacy", 0)); }
        }
        Ok(())
    }

    pub(super) fn recovery_file(&mut self, path: &CStr, step: &'static str) -> Result<bool, Refused> {
        match self.card.entry(path).map_err(|code| Refused::Recovery(step, code))? {
            Entry::Missing => Ok(false),
            Entry::File => Ok(true),
            Entry::Folder | Entry::Other => Err(Refused::Recovery(step, 0)),
        }
    }
}
