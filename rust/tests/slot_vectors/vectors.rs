//! The Rust card installer and firmware switch held to the shared X3 slot
//! vectors (installer/test/x3-slot-vectors.json, from tools/x3_slots.py), which
//! the Python tools and the install page already pass. tests/slot_vectors_test.py
//! flattens the vectors into the file NEWT_SLOT_VECTORS names, one tab-separated
//! row each, and runs this with its negative controls.
use std::ffi::CStr;

use newt_core::update_card::slot_installer::identity::{
    OTA_SECTOR_BYTES, OtaRecord, OtaSelection, STATE_NEW, STATE_VALID, app_image_end, ota_boot_record, ota_boot_slot,
    ota_select, ota_seq_crc,
};
use newt_core::update_card::slot_installer::selection::OtaFlash;
use newt_core::update_card::slot_installer::{
    InstallConsent, InstallIssue, Ports, SECTOR_BYTES, SlotInstaller, check_update_image, classify_slot,
};
use newt_core::update_card::{
    CardImage, Directory, FileInfo, Files, FirmwareName, OFFERS, SlotImage, UpdateOffer, WINDOW_BYTES, Yield,
    firmware_name, read_card_offers,
};
use newt_core::update_manifest::MAX_BYTES;
use newt_core::update_online::Hash;

/// #168's card-install cases, each by the shared vector that holds it.
const CARD_CASES: [(&str, &str); 8] = [
    ("a valid foreign image", "valid"),
    ("the wrong chip", "chip"),
    ("a truncated file", "truncated"),
    ("a bad checksum", "checksum"),
    ("a bad SHA-256", "digest"),
    ("a file larger than its slot", "digest beyond slot"),
    // #368: only erased padding may follow the image.
    ("an image with erased padding", "erased padding"),
    ("an image with other bytes after it", "extra data"),
];

fn rows(kind: &str) -> Vec<Vec<String>> {
    let path = std::env::var("NEWT_SLOT_VECTORS").expect("NEWT_SLOT_VECTORS: run tests/slot_vectors_test.py");
    let rows: Vec<Vec<String>> = std::fs::read_to_string(path)
        .expect("the flattened vectors")
        .lines()
        .map(|line| line.split('\t').map(String::from).collect())
        .filter(|row: &Vec<String>| row[0] == kind)
        .collect();
    assert!(!rows.is_empty(), "no {kind} vectors");
    rows
}

fn bytes(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex")).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct Image {
    name: String,
    data: Vec<u8>,
    slot_bytes: u32,
    intact: bool,
    /// Bytes other than erased padding follow the image: a card file is refused.
    extra_data: bool,
    /// tools/x3_slots.py's image_length.
    length: Option<u64>,
}

fn images() -> Vec<Image> {
    rows("image")
        .into_iter()
        .map(|row| Image {
            name: row[1].clone(),
            data: bytes(&row[2]),
            slot_bytes: row[3].parse().expect("slot bytes"),
            intact: row[4] == "1",
            extra_data: row[5] == "1",
            length: row[6].parse().ok(),
        })
        .collect()
}

/// Buffers the image and hashes it at the end; the device streams to its SDK.
#[derive(Default)]
struct Sha256(Vec<u8>);

impl Hash for Sha256 {
    fn start(&mut self) -> bool {
        self.0.clear();
        true
    }
    fn update(&mut self, bytes: &[u8]) -> bool {
        self.0.extend_from_slice(bytes);
        true
    }
    fn finish(&mut self, digest: &mut [u8; 32]) -> bool {
        *digest = sha256(&self.0);
        true
    }
}

/// FIPS 180-4 SHA-256; the vectors' valid images prove it.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut state: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    message.push(0x80);
    message.resize((message.len() + 8).next_multiple_of(64) - 8, 0);
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (word, chunk) in w.iter_mut().zip(block.chunks(4)) {
            *word = u32::from_be_bytes(chunk.try_into().expect("4 bytes"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = state;
        for (k, w) in K.iter().zip(w) {
            let [a, b, c, d, e, f, g, h] = v;
            let t1 = h
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(*k)
                .wrapping_add(w);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            v = [t1.wrapping_add(t2), a, b, c, d.wrapping_add(t1), e, f, g];
        }
        for (word, add) in state.iter_mut().zip(v) {
            *word = word.wrapping_add(add);
        }
    }
    let mut digest = [0; 32];
    for (chunk, word) in digest.chunks_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// Every mismatch in a test, so a failure names each vector a broken rule breaks.
#[derive(Default)]
struct Mismatches(Vec<String>);

impl Mismatches {
    fn check<T: PartialEq + std::fmt::Debug>(&mut self, name: &str, what: &str, got: T, expected: T) {
        if got != expected {
            self.0.push(format!("{name}: {what} {got:?}, expected {expected:?}"));
        }
    }
    fn finish(self) {
        assert!(self.0.is_empty(), "{} mismatches:\n{}", self.0.len(), self.0.join("\n"));
    }
}

#[test]
fn image_check_matches_every_vector() {
    let mut mismatches = Mismatches::default();
    for image in images() {
        // A read past the data fails, as a failed flash read does; one past
        // the slot would read the next partition, so none may happen.
        let mut read_past_slot = false;
        let mut read = |at: u64, out: &mut [u8]| {
            let end = at + out.len() as u64;
            read_past_slot |= end > u64::from(image.slot_bytes);
            let Some(bytes) = image.data.get(at as usize..end as usize) else { return false };
            out.copy_from_slice(bytes);
            true
        };
        let end = app_image_end(&mut read, image.slot_bytes, &mut Sha256::default());
        mismatches.check(&image.name, "intact", end.is_some(), image.intact);
        if image.intact {
            mismatches.check(&image.name, "end", end, image.length);
        }
        mismatches.check(&image.name, "read past the slot", read_past_slot, false);
    }
    mismatches.finish();
}

/// One card file in /Newt/Update/ and no Newt manifest.
struct Card {
    data: Vec<u8>,
    at: usize,
    listed: bool,
}

const CARD_FILE: &CStr = c"/newt-sd/Newt/Update/vector.bin";

impl Files for Card {
    fn open(&mut self, path: &CStr) -> i32 {
        if path == CARD_FILE { 3 } else { -1 }
    }
    fn stat(&mut self, _: i32) -> Option<FileInfo> {
        Some(FileInfo { regular: true, size: self.data.len() as i64 })
    }
    fn read(&mut self, _: i32, out: &mut [u8]) -> i32 {
        let count = out.len().min(self.data.len() - self.at);
        out[..count].copy_from_slice(&self.data[self.at..self.at + count]);
        self.at += count;
        count as i32
    }
    fn seek(&mut self, _: i32, at: u32) -> i64 {
        if at as usize > self.data.len() {
            return -1;
        }
        self.at = at as usize;
        i64::from(at)
    }
    fn close(&mut self, _: i32) {
        self.at = 0;
    }
}

impl Directory for Card {
    fn open(&mut self, _: &CStr) -> bool {
        self.listed = false;
        true
    }
    fn next(&mut self) -> Option<&CStr> {
        (!std::mem::replace(&mut self.listed, true)).then_some(c"vector.bin")
    }
    fn close(&mut self) {}
}

struct NoYield;
impl Yield for NoYield {
    fn yield_now(&mut self) {}
}

/// The card holding only `data`, and the offer read_card_offers makes of it.
fn card_offer(data: &[u8]) -> (Card, UpdateOffer) {
    let mut files = Card { data: data.to_vec(), at: 0, listed: false };
    let mut directory = Card { data: Vec::new(), at: 0, listed: false };
    let mut offers = [UpdateOffer::default(); OFFERS];
    let (mut more, mut scratch) = (false, [0; MAX_BYTES + 1]);
    read_card_offers(&mut files, &mut directory, &mut offers, &mut more, &mut scratch);
    assert!(offers[1].present, "the card file is offered");
    (files, offers[1])
}

/// The offer read_card_offers makes of `data`, and the bytes the install check
/// would write for a slot of `slot_bytes`, or None when it refuses the file.
fn card_install(data: &[u8], slot_bytes: u32) -> (UpdateOffer, Option<u32>) {
    let (mut files, offer) = card_offer(data);
    let mut window = [0; WINDOW_BYTES];
    let mut yielding = NoYield;
    let mut source = CardImage::new(&mut files, &mut window, &mut yielding);
    let (mut digest, mut order, mut bytes) = ([0; 32], 0, 0);
    let accepted = offer.readable
        && source.open(&offer)
        && check_update_image(&mut source, &offer.manifest, &mut Sha256::default(), slot_bytes, None, &mut digest,
            &mut order, &mut bytes) == InstallIssue::None;
    (offer, accepted.then_some(bytes))
}

#[test]
fn card_install_accepts_only_intact_images() {
    let mut mismatches = Mismatches::default();
    let images = images();
    for image in &images {
        let (_, written) = card_install(&image.data, image.slot_bytes);
        if written.is_some() && !image.intact {
            mismatches.0.push(format!("{}: the card install accepts an image that is not intact", image.name));
        }
    }
    for (case, name) in CARD_CASES {
        let image = images.iter().find(|image| image.name == name).unwrap_or_else(|| panic!("{case}: no {name} vector"));
        let (offer, written) = card_install(&image.data, image.slot_bytes);
        let installs = image.intact && !image.extra_data;
        mismatches.check(name, &format!("{case} installs"), written.is_some(), installs);
        if let Some(bytes) = written {
            mismatches.check(name, "named", offer.name, FirmwareName::CrossInk);
            mismatches.check(name, "writes its image alone", Some(u64::from(bytes)), image.length);
        }
    }
    mismatches.finish();
}

const ERASED_RECORD: OtaRecord = OtaRecord { seq: u32::MAX, label: [255; 20], state: u32::MAX, crc: u32::MAX };

/// The two otadata records, one per sector.
struct Otadata([OtaRecord; 2]);

impl OtaFlash for Otadata {
    fn read(&mut self, at: u32, record: &mut OtaRecord) -> bool {
        *record = self.0[(at / OTA_SECTOR_BYTES) as usize];
        true
    }
    fn erase(&mut self, at: u32) -> bool {
        self.0[(at / OTA_SECTOR_BYTES) as usize] = ERASED_RECORD;
        true
    }
    fn write(&mut self, at: u32, record: &OtaRecord) -> bool {
        self.0[(at / OTA_SECTOR_BYTES) as usize] = *record;
        true
    }
}

/// Both app slots and otadata; slot 0 runs, boots and is confirmed.
struct Flash {
    slots: [Vec<u8>; 2],
    ota: Otadata,
    /// The byte count the last progress report named.
    total: u32,
}

impl Ports for Flash {
    fn running(&mut self) -> i32 {
        0
    }
    fn running_pending(&mut self) -> bool {
        false
    }
    fn selection_unconfirmed(&mut self) -> bool {
        false
    }
    fn mark_selection_unconfirmed(&mut self) {}
    fn slot_bytes(&mut self) -> u32 {
        self.slots[0].len() as u32
    }
    fn read_slot(&mut self, slot: i32, at: u32, out: &mut [u8]) -> bool {
        let Some(bytes) = self.slots[slot as usize].get(at as usize..at as usize + out.len()) else { return false };
        out.copy_from_slice(bytes);
        true
    }
    fn erase_slot(&mut self, slot: i32, at: u32, count: u32) -> bool {
        self.slots[slot as usize][at as usize..(at + count) as usize].fill(0xff);
        true
    }
    fn write_slot(&mut self, slot: i32, at: u32, bytes: &[u8]) -> bool {
        self.slots[slot as usize][at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
        true
    }
    fn ota(&mut self) -> &mut dyn OtaFlash {
        &mut self.ota
    }
    fn progress(&mut self, _: u32, total: u32) {
        self.total = total;
    }
}

/// #368: a card file with erased padding installs as its image alone, as the
/// install page writes it: the write and its readback span only the image.
#[test]
fn card_install_writes_only_the_image() {
    let image = images().into_iter().find(|image| image.name == "erased padding").expect("the erased padding vector");
    let length = image.length.expect("its image length") as usize;
    let (mut files, offer) = card_offer(&image.data);
    let (mut window, mut yielding) = ([0; WINDOW_BYTES], NoYield);
    let mut source = CardImage::new(&mut files, &mut window, &mut yielding);
    assert!(source.open(&offer), "the card file opens");
    let running = OtaRecord { seq: 1, label: [255; 20], state: STATE_VALID, crc: ota_seq_crc(1) };
    let old = vec![0x5a; 2 * SECTOR_BYTES];
    let mut flash = Flash { slots: [old.clone(), old], ota: Otadata([running, ERASED_RECORD]), total: 0 };
    let (mut first, mut buffer) = ([0; SECTOR_BYTES], [0; SECTOR_BYTES]);
    let issue = SlotInstaller::new(&mut flash, &mut source, &mut Sha256::default(), &mut first, &mut buffer)
        .install(&offer.manifest, None, InstallConsent { confirmed: true, reinstall: false, ..InstallConsent::default() });
    let mut mismatches = Mismatches::default();
    mismatches.check(&image.name, "install", issue, InstallIssue::None);
    mismatches.check(&image.name, "bytes written", flash.total as usize, length);
    mismatches.check(&image.name, "slot 1 holds the image", &flash.slots[1][..length], &image.data[..length]);
    mismatches.finish();
}

#[test]
fn slot_names_match_every_head() {
    let mut mismatches = Mismatches::default();
    for row in rows("head") {
        let head: [u8; 512] = bytes(&row[2])[..512].try_into().expect("a 512-byte head");
        let kind = match classify_slot(&head) {
            SlotImage::Empty => "empty",
            SlotImage::Invalid => "invalid",
            SlotImage::Other => "other",
            SlotImage::Newt => "newt",
        };
        mismatches.check(&row[1], "kind", kind, &row[3]);
        // Only another firmware is named; Python shows the rest as ''.
        if kind == "other" {
            let shown = match firmware_name(&head) {
                FirmwareName::CrossInk => "CrossInk",
                FirmwareName::CrossPoint => "CrossPoint",
                FirmwareName::Unknown => "the other firmware",
            };
            mismatches.check(&row[1], "shown", shown, &row[4]);
        }
    }
    mismatches.finish();
}

#[test]
fn boot_and_switch_records_match_every_selection() {
    let mut mismatches = Mismatches::default();
    for row in rows("selection") {
        let name = &row[1];
        let record = |hex: &str| OtaRecord::decode(&bytes(hex).try_into().expect("a 32-byte record"));
        let records = [record(&row[2]), record(&row[3])];
        let boot = format!("{}:{}", ota_boot_record(&records), ota_boot_slot(&records));
        mismatches.check(name, "boot record:slot", if boot == "-1:-1" { "-" } else { &boot }, &row[4]);
        for slot in 0..2 {
            let (mut into, mut entry) = (-1, OtaRecord::default());
            let selection = match ota_select(&records, slot, &mut into, &mut entry, STATE_NEW) {
                OtaSelection::Write => {
                    let mut encoded = [0; 32];
                    entry.encode(&mut encoded);
                    format!("{into}:{}", hex(&encoded))
                }
                OtaSelection::Selected => "selected".into(),
                OtaSelection::Unknown => "unknown".into(),
                OtaSelection::Exhausted => "exhausted".into(),
            };
            mismatches.check(name, &format!("select slot {slot}"), &selection, &row[5 + slot as usize]);
        }
    }
    mismatches.finish();
}
