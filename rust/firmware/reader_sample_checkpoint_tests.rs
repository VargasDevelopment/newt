// SPDX-License-Identifier: MIT
//! Actual ProgressStore transactions through the sample checkpoint arm.
extern crate std;
use super::*;
use newt_core::{durable::{BlobRead, DurableWrite, StorageOpen}, progress::{BYTES, NAMESPACE}};
use std::{string::String, vec::Vec};

#[derive(Default)]
struct Store {
    slots: [Option<[u8; BYTES]>; 2],
    opened: Vec<(String, bool)>,
    ends: usize,
    fail_open: bool,
}
impl Backend for Store {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        self.opened.push((String::from(namespace), readonly));
        if self.fail_open { StorageOpen::Failed } else { StorageOpen::Opened }
    }
    fn end(&mut self) { self.ends += 1; }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        let at = newt_core::durable::SLOT_KEYS.iter().position(|&slot| slot == key).unwrap();
        match self.slots[at] {
            None => BlobRead::Missing,
            Some(bytes) => {
                let count = output.len().min(bytes.len());
                output[..count].copy_from_slice(&bytes[..count]);
                BlobRead::Present(bytes.len())
            }
        }
    }
    fn put_bytes(&mut self, _permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        let at = newt_core::durable::SLOT_KEYS.iter().position(|&slot| slot == key).unwrap();
        self.slots[at] = Some(bytes.try_into().unwrap());
        bytes.len()
    }
    fn readback_durable(&self) -> bool { true }
}
fn candidate(character: u32) -> Progress {
    Progress { book: 0x5e4c9375286ce1cb,
        locator: newt_core::progress::Locator { spine: 1, character },
        active_milliseconds: 32000, forward_pages: 8, session_milliseconds: 17000,
        ..Progress::default() }
}

#[test]
fn sample_uses_only_nvs_progress_and_keeps_destination_unchanged() {
    let mut store = Store::default();
    let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
    let candidate = candidate(100);
    let result = checkpoint.save_sample(&mut store, candidate, true);
    assert_eq!(result, CheckpointResult { progress: Outcome::Verified, destination: Outcome::Unchanged });
    assert_eq!(store.opened, [(String::from(NAMESPACE), false)]);
    assert_eq!(store.ends, 1);
    let acknowledged = checkpoint.acknowledged_sample(candidate.book).unwrap();
    assert_eq!(acknowledged.locator, candidate.locator);
    assert_eq!(acknowledged.session_milliseconds, candidate.session_milliseconds);
    assert!(checkpoint.acknowledged_for(&[0; 32]).is_none());
    let durable = newt_core::progress_store::ProgressStore::new(&mut store).load();
    assert_eq!(durable, Load::Loaded(Progress { session_milliseconds: 0, ..acknowledged }));
}

#[test]
fn unavailable_sample_progress_does_no_storage_work() {
    let mut store = Store::default();
    let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
    let result = checkpoint.save_sample(&mut store, candidate(100), false);
    assert_eq!(result, CheckpointResult { progress: Outcome::Unconfirmed, destination: Outcome::Unchanged });
    assert!(store.opened.is_empty());
    assert_eq!(store.ends, 0);
    assert!(checkpoint.acknowledged().is_none());
}

#[test]
fn failed_sample_save_keeps_previous_acknowledgement_and_reports_uncertainty() {
    let mut store = Store::default();
    let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
    checkpoint.save_sample(&mut store, candidate(100), true);
    let before = checkpoint.acknowledged();
    store.fail_open = true;
    let result = checkpoint.save_sample(&mut store, candidate(200), true);
    assert_eq!(result.progress, Outcome::Unconfirmed);
    assert_eq!(result.destination, Outcome::Unchanged);
    assert_eq!(checkpoint.acknowledged(), before);
    assert!(checkpoint.page_unconfirmed());
    assert_eq!(checkpoint.notice(), Some("Save unconfirmed. Restart may use old or new page."));
}

#[test]
fn external_digest_acknowledgement_is_not_a_sample_acknowledgement() {
    let mut checkpoint = ReaderCheckpoint::new(CheckpointRestore::Latest);
    checkpoint.acknowledged = Some(candidate(100));
    checkpoint.acknowledged_digest = Some([7; 32]);
    assert!(checkpoint.acknowledged_sample(candidate(100).book).is_none());
    assert_eq!(checkpoint.acknowledged_for(&[7; 32]), checkpoint.acknowledged());
}
