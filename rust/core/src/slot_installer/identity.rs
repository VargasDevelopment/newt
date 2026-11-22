//! Missing pure app_slot_identity.h leaves; the existing card classifier stays the sole classifier.
use core::ffi::CStr;
use super::{Hash, SlotImage};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NewtAppIdentity { pub version: [u8;24], pub build: [u8;16] }
pub fn exact_build_id(id: Option<&CStr>) -> bool {
    id.is_some_and(|id| id.to_bytes().len() == 12 && id.to_bytes().iter().all(|&v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v)))
}
pub fn newt_app_identity(version: &CStr, build: Option<&CStr>) -> NewtAppIdentity {
    let mut identity = NewtAppIdentity::default();
    let count = version.to_bytes().len().min(23);
    identity.version[..count].copy_from_slice(&version.to_bytes()[..count]);
    if exact_build_id(build) {
        if let Some(build) = build { identity.build[..12].copy_from_slice(build.to_bytes()); }
    }
    identity
}
pub(super) fn newt_app_identity_fields(version: &CStr, build: &CStr) -> NewtAppIdentity {
    let mut identity = NewtAppIdentity::default();
    identity.version[..version.to_bytes().len()].copy_from_slice(version.to_bytes());
    identity.build[..build.to_bytes().len()].copy_from_slice(build.to_bytes());
    identity
}
pub fn read_newt_identity(head: &[u8;512], identity: &mut NewtAppIdentity) -> bool {
    identity.version.copy_from_slice(&head[304..328]);
    identity.build.copy_from_slice(&head[328..344]);
    identity.version[23] == 0 && identity.build[15] == 0
}
pub fn other_firmware_slot(app0: SlotImage, app1: SlotImage, running: i32) -> i32 {
    if app0 == SlotImage::Invalid || app1 == SlotImage::Invalid { return -1; }
    let in0 = app0 == SlotImage::Other;
    let in1 = app1 == SlotImage::Other;
    let slot = if in0 == in1 { -1 } else if in0 { 0 } else { 1 };
    if slot == running { -1 } else { slot }
}
pub const OTA_SECTOR_BYTES: u32 = 4096;
pub const STATE_NEW: u32 = 0;
pub const STATE_PENDING: u32 = 1;
pub const STATE_VALID: u32 = 2;
pub const STATE_INVALID: u32 = 3;
pub const STATE_ABORTED: u32 = 4;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OtaRecord { pub seq: u32, pub label: [u8;20], pub state: u32, pub crc: u32 }
impl OtaRecord {
    pub fn encode(&self, bytes: &mut [u8;32]) {
        bytes[..4].copy_from_slice(&self.seq.to_le_bytes());bytes[4..24].copy_from_slice(&self.label);
        bytes[24..28].copy_from_slice(&self.state.to_le_bytes());bytes[28..].copy_from_slice(&self.crc.to_le_bytes());
    }
    pub fn decode(bytes: &[u8;32]) -> Self {
        let word = |at: usize| u32::from_le_bytes([bytes[at],bytes[at+1],bytes[at+2],bytes[at+3]]);
        let mut record = Self { seq: word(0), label: [0;20], state: word(24), crc: word(28) };
        record.label.copy_from_slice(&bytes[4..24]);record
    }
}
pub fn ota_seq_crc(seq: u32) -> u32 {
    let mut crc = 0u32;
    for byte in 0..4 {
        crc ^= (seq >> (8 * byte)) & 255;
        for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1)); }
    }
    !crc
}
pub fn ota_record_usable(record: &OtaRecord) -> bool {
    record.seq != 0 && record.seq != u32::MAX && record.crc == ota_seq_crc(record.seq) && record.state != STATE_INVALID && record.state != STATE_ABORTED
}
pub fn ota_boot_record(records: &[OtaRecord;2]) -> i32 {
    let usable = [ota_record_usable(&records[0]),ota_record_usable(&records[1])];
    if !usable[0] && !usable[1] { return -1; }
    let followed = if !usable[1] || (usable[0] && records[0].seq >= records[1].seq) { 0 } else { 1 };
    if records[followed].state != STATE_PENDING { return followed as i32; }
    let fallback = 1 - followed;
    if usable[fallback] && records[fallback].state != STATE_PENDING { fallback as i32 } else { -1 }
}
pub fn ota_boot_slot(records: &[OtaRecord;2]) -> i32 {
    let record = ota_boot_record(records);
    if record < 0 { -1 } else { (records[record as usize].seq.wrapping_sub(1) % 2) as i32 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum OtaSelection { Write, Selected, Unknown, Exhausted }
pub fn ota_select(records: &[OtaRecord;2], slot: i32, into: &mut i32, entry: &mut OtaRecord, state: u32) -> OtaSelection {
    let current = ota_boot_record(records);
    if current < 0 { return OtaSelection::Unknown; }
    if (records[current as usize].seq.wrapping_sub(1) % 2) as i32 == slot { return OtaSelection::Selected; }
    let mut seq = records[current as usize].seq.wrapping_add(1);
    if (seq.wrapping_sub(1) % 2) as i32 != slot { seq = seq.wrapping_add(1); }
    if seq == u32::MAX { return OtaSelection::Exhausted; }
    *entry = OtaRecord { seq, label: [255;20], state, crc: ota_seq_crc(seq) };
    *into = 1 - current;
    OtaSelection::Write
}
pub fn ota_rolled_back(records: &[OtaRecord;2], running: i32) -> bool {
    let current = ota_boot_record(records);
    if current < 0 || (records[current as usize].seq.wrapping_sub(1) % 2) as i32 != running { return false; }
    let other = &records[1-current as usize];
    other.seq != 0 && other.seq != u32::MAX && other.crc == ota_seq_crc(other.seq) && other.seq > records[current as usize].seq &&
        (other.seq.wrapping_sub(1) % 2) as i32 != running && matches!(other.state, STATE_ABORTED | STATE_INVALID)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageState { Settled, Pending, Unreadable }
pub fn segment_placed(load: u32, data_at: u64, length: u32) -> bool {
    let mapped = (0x3c000000..0x3c800000).contains(&load) || (0x42000000..0x42800000).contains(&load);
    length % 4 == 0 && (!mapped || u64::from(load % 65536) == data_at % 65536)
}
pub fn app_image_intact(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, slot_bytes: u32, hash: &mut dyn Hash) -> bool {
    app_image_end(read, slot_bytes, hash).is_some()
}
/// Where an intact image ends, through its appended SHA-256 when it has one.
pub fn app_image_end(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, slot_bytes: u32, hash: &mut dyn Hash) -> Option<u64> {
    let mut header = [0;24];let mut chunk = [0;256];
    if slot_bytes < 24 || !read(0,&mut header) { return None; }
    let chip = u16::from_le_bytes([header[12],header[13]]);
    let appended = header[23] == 1;
    if header[0] != 0xe9 || header[1] == 0 || header[1] > 16 || chip != 5 || header[23] > 1 || !hash.start() || !hash.update(&header) { return None; }
    let mut at = 24u64;
    let mut checksum = 0xef;
    for _ in 0..header[1] {
        let mut entry = [0;8];
        if at + 8 > u64::from(slot_bytes) || !read(at,&mut entry) || !hash.update(&entry) { return None; }
        at += 8;
        let load = u32::from_le_bytes([entry[0],entry[1],entry[2],entry[3]]);
        let length = u32::from_le_bytes([entry[4],entry[5],entry[6],entry[7]]);
        if at + u64::from(length) > u64::from(slot_bytes) || !segment_placed(load,at,length) { return None; }
        let end = at + u64::from(length);
        while at < end {
            let count = (end-at).min(256) as usize;
            if !read(at,&mut chunk[..count]) || !hash.update(&chunk[..count]) { return None; }
            for byte in &chunk[..count] { checksum ^= byte; }
            at += count as u64;
        }
    }
    let digest_at = (at / 16 + 1) * 16;
    let tail = (digest_at-at) as usize;
    let mut digest = [0;32];let mut stored = [0;32];
    if digest_at + (if appended { 32 } else { 0 }) > u64::from(slot_bytes) || !read(at,&mut chunk[..tail]) || chunk[tail-1] != checksum { return None; }
    let intact = !appended || (hash.update(&chunk[..tail]) && hash.finish(&mut digest) && read(digest_at,&mut stored) && digest == stored);
    intact.then_some(digest_at + if appended { 32 } else { 0 })
}
