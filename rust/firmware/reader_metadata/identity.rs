// SPDX-License-Identifier: MIT
//! firmware/chapter_identity_record.h: one checked three-slot snapshot.
use super::IdentityStorage;
use newt_core::{cache_names::identity::{self, Key}, progress::Locator,
    progress_store::PeerLocators, utf8, wire::{get32, put32}};
use newt_reader_navigation::{Section, TITLE_BYTES};

pub const BYTES: usize = 60 + TITLE_BYTES;
const KEY_BYTES: usize = core::mem::size_of::<Key>();
const LOAD_STATUS: usize = KEY_BYTES + BYTES;
pub const LOAD_SCRATCH_BYTES: usize = LOAD_STATUS + 5;
const SNAPSHOT_STATE: usize = KEY_BYTES + 3 * BYTES;
const FAILED: usize = SNAPSHOT_STATE + 3;
const NEXT_GENERATION: usize = FAILED + 1;
const STOP_STATE: usize = NEXT_GENERATION + 4;
pub const CHECKPOINT_SCRATCH_BYTES: usize = STOP_STATE + core::mem::size_of::<Option<&'static dyn Fn() -> bool>>();
const POLICY: u32 = 1;

pub struct Record<'a, S: IdentityStorage> {
    store: &'a mut S,
    scratch: &'a mut [u8],
}
impl<'a, S: IdentityStorage> Record<'a, S> {
    pub fn new(store: &'a mut S, scratch: &'a mut [u8]) -> Option<Self> {
        if scratch.len() < LOAD_SCRATCH_BYTES { return None; }
        scratch[LOAD_STATUS] = 0;
        Some(Self { store, scratch })
    }
    pub fn load_for(&mut self, digest: &[u8; 32], at: Locator, label: &mut [u8; TITLE_BYTES]) -> bool {
        self.scratch[LOAD_STATUS] = 0;
        let mut chosen = None;
        let mut generation = 0;
        for slot in 0..3 {
            if self.read_record(digest, slot) && valid(self.cold(), digest, at)
                && get32(self.cold(), 56) >= generation {
                chosen = Some(slot); generation = get32(self.cold(), 56);
            }
        }
        if self.scratch[LOAD_STATUS] != 0 { return false; }
        let Some(slot) = chosen else { return false; };
        if !self.read_record(digest, slot) || self.scratch[LOAD_STATUS] != 0
            || !valid(self.cold(), digest, at) || get32(self.cold(), 56) != generation { return false; }
        let length = get32(self.cold(), 20) as usize;
        label[..length + 1].copy_from_slice(&self.cold()[60..60 + length + 1]);
        true
    }
    fn cold(&self) -> &[u8] { &self.scratch[KEY_BYTES..KEY_BYTES + BYTES] }
    fn key(&mut self, digest: &[u8; 32], slot: usize) {
        if let Some(key) = self.scratch.first_chunk_mut::<KEY_BYTES>() {
            identity::make(digest, slot as u32, key);
        }
    }
    fn read_record(&mut self, digest: &[u8; 32], slot: usize) -> bool {
        self.key(digest, slot);
        let (key, rest) = self.scratch.split_at_mut(KEY_BYTES);
        let mut uncertain = false;
        let found = self.store.inspect_identity(key, &mut rest[..BYTES], &mut uncertain);
        if uncertain { rest[BYTES] = 1; }
        found
    }
    fn record(&self, slot: usize) -> &[u8] {
        &self.scratch[KEY_BYTES + slot * BYTES..KEY_BYTES + (slot + 1) * BYTES]
    }
    fn find_snapshot(&self, digest: &[u8; 32], at: Locator) -> Option<usize> {
        let mut chosen = None;
        let mut generation = 0;
        for slot in 0..3 {
            if self.scratch[SNAPSHOT_STATE + slot] != 0 && valid(self.record(slot), digest, at)
                && get32(self.record(slot), 56) >= generation {
                chosen = Some(slot); generation = get32(self.record(slot), 56);
            }
        }
        chosen
    }
    fn newest_generation(&self, digest: &[u8; 32]) -> u32 {
        let mut generation = 0;
        for slot in 0..3 {
            let record = self.record(slot);
            let at = Locator { spine: get32(record, 12), character: get32(record, 16) };
            if self.scratch[SNAPSHOT_STATE + slot] != 0 && valid(record, digest, at) {
                generation = generation.max(get32(record, 56));
            }
        }
        generation
    }
    fn take_snapshot(&mut self, digest: &[u8; 32], stop: &dyn Fn() -> bool) -> bool {
        for slot in 0..3 {
            if stop() { return false; }
            self.key(digest, slot);
            let (key, rest) = self.scratch.split_at_mut(KEY_BYTES);
            let mut uncertain = false;
            let found = self.store.inspect_identity(key, &mut rest[slot * BYTES..(slot + 1) * BYTES], &mut uncertain);
            self.scratch[SNAPSHOT_STATE + slot] = u8::from(found);
            if uncertain { self.scratch[FAILED] = 1; }
            if self.scratch[FAILED] != 0 { return false; }
        }
        true
    }
    pub fn prepare_checkpoint<'label>(&mut self, digest: &[u8; 32], at: Locator, section: Section<'_>,
        peers: &PeerLocators, mut section_at: impl FnMut(Locator) -> Section<'label>, stop: &dyn Fn() -> bool,
    ) -> bool {
        if self.scratch.len() < CHECKPOINT_SCRATCH_BYTES { return false; }
        self.scratch[FAILED] = 0;
        if !self.take_snapshot(digest, stop) { return false; }
        let newest = self.newest_generation(digest);
        if self.scratch[FAILED] != 0 || newest == u32::MAX { return false; }
        put32(self.scratch, NEXT_GENERATION, newest + 1);
        for slot in 0..2 {
            if peers.present & (1 << slot) != 0 {
                if stop() { return false; }
                self.bind_peer(digest, slot, peers.slots[slot], &mut section_at, stop);
                if self.scratch[FAILED] != 0 || stop() { return false; }
            }
        }
        if stop() { return false; }
        if let Some(latest) = self.find_snapshot(digest, at) {
            let label = cstr(section.label);
            if label.is_empty() || cstr(&self.record(latest)[60..]) == label {
                if latest == 2 { return true; }
                self.copy_record(latest, 2);
                return self.write_snapshot(digest, 2);
            }
        }
        self.save_slot(digest, 2, at, section)
    }
    fn copy_record(&mut self, from: usize, to: usize) {
        self.scratch.copy_within(KEY_BYTES + from * BYTES..KEY_BYTES + (from + 1) * BYTES,
            KEY_BYTES + to * BYTES);
    }
    fn bind_peer<'label>(&mut self, digest: &[u8; 32], slot: usize, at: Locator,
        section_at: &mut impl FnMut(Locator) -> Section<'label>, stop: &dyn Fn() -> bool,
    ) -> bool {
        let bound_generation = if self.scratch[SNAPSHOT_STATE + slot] != 0 && valid(self.record(slot), digest, at) {
            get32(self.record(slot), 56)
        } else { 0 };
        if let Some(source) = self.find_snapshot(digest, at) {
            if get32(self.record(source), 56) == bound_generation { return true; }
            self.copy_record(source, slot);
            if stop() || !self.write_snapshot(digest, slot) { return false; }
        } else {
            let section = section_at(at);
            if !cstr(section.label).is_empty() && (stop() || !self.save_slot(digest, slot, at, section)) { return false; }
        }
        true
    }
    fn save_slot(&mut self, digest: &[u8; 32], slot: usize, at: Locator, section: Section<'_>) -> bool {
        let generation = get32(self.scratch, NEXT_GENERATION);
        let label = cstr(section.label);
        if slot >= 3 || label.is_empty() || label.len() >= TITLE_BYTES { return false; }
        if self.scratch[SNAPSHOT_STATE + slot] != 0 && valid(self.record(slot), digest, at)
            && get32(self.record(slot), 56) == generation && cstr(&self.record(slot)[60..]) == label { return true; }
        let record = &mut self.scratch[KEY_BYTES + slot * BYTES..KEY_BYTES + (slot + 1) * BYTES];
        record.fill(0);
        if !newt_reader_navigation::copy_label(Some(section.label), &mut record[60..]) { return false; }
        record[..8].copy_from_slice(b"NTCI0002");
        put32(record, 8, POLICY); put32(record, 12, at.spine); put32(record, 16, at.character);
        put32(record, 20, label.len() as u32); put32(record, 56, generation);
        record[24..56].copy_from_slice(digest);
        self.write_snapshot(digest, slot)
    }
    fn write_snapshot(&mut self, digest: &[u8; 32], slot: usize) -> bool {
        if self.scratch[FAILED] != 0 { return false; }
        self.key(digest, slot);
        let good = self.store.begin_write(&self.scratch[..KEY_BYTES])
            && self.store.write(&self.scratch[KEY_BYTES + slot * BYTES..KEY_BYTES + (slot + 1) * BYTES])
            && self.store.end_write();
        self.scratch[SNAPSHOT_STATE + slot] = u8::from(good);
        if !good && !self.store.remove(&self.scratch[..KEY_BYTES]) { self.scratch[FAILED] = 1; }
        good
    }
}
fn cstr(bytes: &[u8]) -> &[u8] { &bytes[..bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len())] }
fn valid(record: &[u8], digest: &[u8; 32], at: Locator) -> bool {
    let length = get32(record, 20) as usize;
    if &record[..8] != b"NTCI0002" || get32(record, 8) != POLICY || get32(record, 56) == 0
        || get32(record, 12) != at.spine || get32(record, 16) != at.character || &record[24..56] != digest
        || length == 0 || length >= TITLE_BYTES || record[60 + length] != 0 { return false; }
    if record[60 + length + 1..].iter().any(|&byte| byte != 0) { return false; }
    let (mut offset, mut scalar) = (0, 0);
    while offset < length {
        if !utf8::next(Some(&record[60..60 + length]), &mut offset, &mut scalar) || scalar == 0 { return false; }
    }
    true
}
