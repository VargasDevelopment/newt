// SPDX-License-Identifier: MIT
//! Reclaim admission uses the same router and durable file format as the reader.
use super::*;
use newt_core::{external_progress_store::ExternalProgressStore, progress::{self, Progress, Selection}};

impl<L: ScopedTransaction, N: Native, M: StateMedium<N>> ReadingStateRouter<'_, L, N, M> {
    /// Preserve both legacy slots verbatim when creating the file; an existing
    /// SD authority always wins. No NVS key may be erased on a false result.
    #[inline(never)]
    pub fn preserve_legacy_progress(&mut self, name: &str, digest: &[u8; 32], had_progress: bool) -> bool {
        {
            let owner = ExternalProgressStore::new(self, Some(digest));
            if owner.book_id() == 0 || owner.storage_namespace() != name {
                return false;
            }
        }
        if self.begin(name, false) != StorageOpen::Opened {
            self.state.facts.sync_failed = true;
            self.end();
            return false;
        }
        // A readable FAT copy alone is insufficient after a failed sync. Force
        // a modified-handle barrier even when the file was already present.
        let synced = self.state.parts().is_some_and(|(record, io, facts)| {
            let ok = !record.legacy && record.settle(io);
            facts.sync_failed = !ok;
            if !ok { facts.note(io.errno); }
            ok
        });
        self.end();
        if !synced || self.begin(name, true) != StorageOpen::Opened {
            self.end();
            return false;
        }
        let from_sd = self.state.parts().is_some_and(|(record, _, _)| !record.legacy);
        let verified = from_sd && self.verify_progress(digest, had_progress);
        self.end();
        verified
    }

    fn verify_progress(&mut self, digest: &[u8; 32], had_progress: bool) -> bool {
        let mut found = [0; 32];
        if self.read_blob("digest", &mut found) != BlobRead::Present(32) || found != *digest {
            return false;
        }
        let Some(prefix) = digest.first_chunk::<8>() else { return false };
        let book = u64::from_le_bytes(*prefix);
        let mut slots = [[0; progress::BYTES]; 2];
        let mut sizes = [0; 2];
        for (index, key) in SLOT_KEYS.into_iter().enumerate() {
            match self.read_blob(key, &mut slots[index]) {
                BlobRead::Missing => {}
                BlobRead::Present(progress::BYTES)
                    if Progress::decode(&slots[index]).is_some_and(|p| p.book == book) => {
                        sizes[index] = progress::BYTES;
                    }
                _ => return false,
            }
        }
        match progress::select(&slots[0][..sizes[0]], &slots[1][..sizes[1]]) {
            Selection::First(_) | Selection::Second(_) => true,
            Selection::None => !had_progress,
            Selection::Ambiguous => false,
        }
    }
}
