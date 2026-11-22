// SPDX-License-Identifier: MIT
//! Wi-Fi key, save, forget and restore policy. MIT. Caller supplies Secret scopes and native wipe semantics over lent regions.
#![forbid(unsafe_code)]
use core::ffi::CStr;
use crate::durable::{Backend, BlobRead, Commit, Load, RecordStore, StorageOpen, commit_bytes};
use super::{BYTES, NAME_BYTES, PASSWORD_BYTES, PLAIN_BYTES, SealedWifiNetwork, WifiNetworkCodec};
#[path = "workflow/ports.rs"]
mod ports;
pub use ports::{Cipher, KeyBackend, KeyRegion, NativeWipe, Random, ScopedSecrets, Secret, Work};
use ports::BackendLoan;

pub const KEY_NAMESPACE: &str = "newt_wifi";
pub const KEY_NAME: &str = "key";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WifiCommit { Saved, NotSaved, Unconfirmed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WifiKeyRead { Present, Missing, Failed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum RecordLoad { Loaded, Empty, Ambiguous, Failed }
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct WifiChoice { pub saved: bool, pub name: [u8; NAME_BYTES + 1] }
impl Default for WifiChoice { fn default() -> Self { Self { saved: false, name: [0; NAME_BYTES + 1] } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiChangeReport { pub record: WifiCommit, pub key_written: bool, pub key: WifiCommit }
impl Default for WifiChangeReport { fn default() -> Self { Self { record: WifiCommit::NotSaved, key_written: false, key: WifiCommit::Saved } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiRestoreReport { pub record: RecordLoad, pub network: bool, pub key: WifiKeyRead, pub opened: bool }
impl Default for WifiRestoreReport { fn default() -> Self { Self { record: RecordLoad::Empty, network: false, key: WifiKeyRead::Missing, opened: false } } }

pub fn read_wifi_key(keys: &mut dyn KeyBackend, key: &mut dyn Secret<32>, wipe: &dyn NativeWipe) -> WifiKeyRead {
    key.clear(wipe);
    match keys.backend().begin(KEY_NAMESPACE, true) {
        StorageOpen::Missing => return WifiKeyRead::Missing,
        StorageOpen::Opened => (),
        _ => return WifiKeyRead::Failed,
    }
    let mut read = BlobRead::Failed;
    key.fill(32, wipe, &mut |bytes| {
        read = keys.backend().read_blob(KEY_NAME, bytes);
        read == BlobRead::Present(bytes.len())
    });
    keys.backend().end();
    if read == BlobRead::Failed { WifiKeyRead::Failed }
    else if key.size() == 32 { WifiKeyRead::Present } else { WifiKeyRead::Missing }
}
pub fn write_wifi_key(keys: &mut dyn KeyBackend, random: &mut dyn Random, key: &mut dyn Secret<32>, observed: KeyRegion<'_>, readback: &mut [u8;32], scopes: &dyn ScopedSecrets, wipe: &dyn NativeWipe) -> WifiCommit {
    if !key.fill(32, wipe, &mut |bytes| random.fill(bytes)) { return WifiCommit::NotSaved; }
    if keys.backend().begin(KEY_NAMESPACE, false) != StorageOpen::Opened {
        key.clear(wipe); return WifiCommit::NotSaved;
    }
    readback.fill(0);
    let commit = commit_bytes(&mut BackendLoan::new(keys.backend(), None), KEY_NAME, key.bytes(), readback);
    keys.backend().end();
    wipe.wipe(readback);
    if commit == Commit::Saved { return WifiCommit::Saved; }
    let mut result = WifiCommit::Unconfirmed;
    if keys.backend().readback_durable() {
        scopes.with_key(observed, wipe, &mut |observed| {
            let read = read_wifi_key(keys, observed, wipe);
            if read == WifiKeyRead::Present && observed.bytes() == key.bytes() { result = WifiCommit::Saved; return; }
            if read != WifiKeyRead::Failed { result = WifiCommit::NotSaved; }
        });
    }
    if result == WifiCommit::Saved { return result; }
    key.clear(wipe);
    result
}
pub fn erase_wifi_key(keys: &mut dyn KeyBackend, observed: KeyRegion<'_>, scopes: &dyn ScopedSecrets, wipe: &dyn NativeWipe) -> WifiCommit {
    if keys.backend().begin(KEY_NAMESPACE, false) != StorageOpen::Opened { return WifiCommit::NotSaved; }
    let erased = keys.erase_key();
    keys.backend().end();
    if !keys.backend().readback_durable() { return WifiCommit::Unconfirmed; }
    let mut read = WifiKeyRead::Failed;
    scopes.with_key(observed, wipe, &mut |observed| {
        read = read_wifi_key(keys, observed, wipe);
        observed.clear(wipe);
    });
    if read == WifiKeyRead::Missing { WifiCommit::Saved }
    else if !erased && read == WifiKeyRead::Present { WifiCommit::NotSaved } else { WifiCommit::Unconfirmed }
}
pub fn commit_wifi_record(card: &mut dyn Backend, sealed: &SealedWifiNetwork) -> WifiCommit {
    let mut wrote = false;
    let saved = RecordStore::<_, WifiNetworkCodec, BYTES>::new(&mut BackendLoan::new(card, Some(&mut wrote))).save(*sealed);
    if saved == Commit::Saved { return WifiCommit::Saved; }
    if !wrote { return WifiCommit::NotSaved; }
    if !card.readback_durable() { return WifiCommit::Unconfirmed; }
    match RecordStore::<_, WifiNetworkCodec, BYTES>::new(&mut BackendLoan::new(card, None)).load() {
        Load::Loaded(observed) if observed == *sealed => WifiCommit::Saved,
        Load::Loaded(_) | Load::Empty => WifiCommit::NotSaved,
        _ => WifiCommit::Unconfirmed,
    }
}
pub fn save_wifi_network(card: &mut dyn Backend, keys: &mut dyn KeyBackend, cipher: &mut dyn Cipher, random: &mut dyn Random, scopes: &dyn ScopedSecrets, wipe: &dyn NativeWipe, name: Option<&CStr>, password: &dyn Secret<PASSWORD_BYTES>, choice: &mut WifiChoice, work: &mut Work<'_>) -> WifiChangeReport {
    let mut report = WifiChangeReport::default();
    let Some(name) = name.filter(|name| !name.to_bytes().is_empty() && name.to_bytes().len() <= NAME_BYTES) else { return report };
    let length = name.to_bytes().len();
    scopes.with_key(work.key.borrow(), wipe, &mut |key| {
        let found = read_wifi_key(keys, key, wipe);
        if found == WifiKeyRead::Failed { return; }
        if found == WifiKeyRead::Missing {
            report.key_written = true;
            report.key = write_wifi_key(keys, random, key, work.observed.borrow(), work.readback, scopes, wipe);
            if report.key != WifiCommit::Saved { return; }
        }
        work.plain.fill(0);
        work.plain[0] = length as u8;
        work.plain[1..1 + length].copy_from_slice(name.to_bytes());
        work.plain[1 + NAME_BYTES] = password.size() as u8;
        work.plain[2 + NAME_BYTES..2 + NAME_BYTES + password.size()].copy_from_slice(&password.bytes()[..password.size()]);
        let mut sealed = SealedWifiNetwork { network: true, ..SealedWifiNetwork::default() };
        let header = WifiNetworkCodec::associated(true);
        let ok = random.fill(&mut sealed.nonce) && cipher.seal(key.bytes(), &sealed.nonce, &header, work.plain, &mut sealed.cipher, &mut sealed.tag);
        wipe.wipe(work.plain);
        key.clear(wipe);
        if !ok { return; }
        report.record = commit_wifi_record(card, &sealed);
        if report.record == WifiCommit::Saved {
            *choice = WifiChoice::default(); choice.saved = true;
            choice.name[..length].copy_from_slice(name.to_bytes());
        }
    });
    report
}
pub fn forget_wifi_network(card: &mut dyn Backend, keys: &mut dyn KeyBackend, scopes: &dyn ScopedSecrets, wipe: &dyn NativeWipe, choice: &mut WifiChoice, work: &mut Work<'_>) -> WifiChangeReport {
    let mut report = WifiChangeReport { record: commit_wifi_record(card, &SealedWifiNetwork::default()), ..WifiChangeReport::default() };
    if report.record != WifiCommit::Saved { return report; }
    *choice = WifiChoice::default();
    scopes.with_key(work.key.borrow(), wipe, &mut |key| {
        if read_wifi_key(keys, key, wipe) == WifiKeyRead::Present {
            report.key_written = true;
            report.key = erase_wifi_key(keys, work.observed.borrow(), scopes, wipe);
        }
        key.clear(wipe);
    });
    report
}
pub fn restore_wifi_network(card: &mut dyn Backend, keys: &mut dyn KeyBackend, cipher: &mut dyn Cipher, scopes: &dyn ScopedSecrets, wipe: &dyn NativeWipe, choice: &mut WifiChoice, mut password: Option<&mut dyn Secret<PASSWORD_BYTES>>, work: &mut Work<'_>) -> WifiRestoreReport {
    let mut report = WifiRestoreReport::default();
    *choice = WifiChoice::default();
    if let Some(password) = password.as_deref_mut() { password.clear(wipe); }
    let sealed = match RecordStore::<_, WifiNetworkCodec, BYTES>::new(&mut BackendLoan::new(card, None)).load() {
        Load::Loaded(sealed) => { report.record = RecordLoad::Loaded; sealed }
        Load::Empty => return report,
        Load::Ambiguous => { report.record = RecordLoad::Ambiguous; return report; }
        Load::Failed => { report.record = RecordLoad::Failed; return report; }
    };
    report.network = sealed.network;
    if !report.network { return report; }
    scopes.with_key(work.key.borrow(), wipe, &mut |key| {
        report.key = read_wifi_key(keys, key, wipe);
        if report.key != WifiKeyRead::Present { return; }
        work.plain.fill(0);
        let header = WifiNetworkCodec::associated(true);
        report.opened = cipher.open(key.bytes(), &sealed.nonce, &header, &sealed.cipher, &sealed.tag, work.plain);
        key.clear(wipe);
        let length = usize::from(work.plain[0]);
        if report.opened && (1..=NAME_BYTES).contains(&length) && usize::from(work.plain[1 + NAME_BYTES]) <= PASSWORD_BYTES && !work.plain[1..1 + length].contains(&0) {
            choice.saved = true;
            choice.name[..length].copy_from_slice(&work.plain[1..1 + length]);
            if let Some(password) = password.as_deref_mut() {
                password.fill(usize::from(work.plain[1 + NAME_BYTES]), wipe, &mut |out| { out.copy_from_slice(&work.plain[2 + NAME_BYTES..2 + NAME_BYTES + out.len()]); true });
            }
        } else { report.opened = false; }
        wipe.wipe(work.plain);
    });
    report
}
