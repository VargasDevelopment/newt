//! Frozen ota_selection.h acknowledgement order; no native flash implementation.
use super::identity::*;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SaveOutcome { Unchanged, Verified, Unconfirmed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum OtaCommit { Switch, Fresh, Settle }
pub trait OtaFlash {
    fn read(&mut self, at: u32, record: &mut OtaRecord) -> bool;
    fn erase(&mut self, at: u32) -> bool;
    fn write(&mut self, at: u32, record: &OtaRecord) -> bool;
}
pub fn read_ota_records(flash: &mut dyn OtaFlash, records: &mut [OtaRecord;2]) -> bool {
    flash.read(0,&mut records[0]) && flash.read(OTA_SECTOR_BYTES,&mut records[1])
}
pub fn commit_ota_selection(flash: &mut dyn OtaFlash, slot: i32, mode: OtaCommit) -> SaveOutcome {
    let mut records = [OtaRecord::default();2];let mut entry = OtaRecord::default();let mut into = 0;
    if !read_ota_records(flash,&mut records) { return SaveOutcome::Unconfirmed; }
    match ota_select(&records,slot,&mut into,&mut entry,if mode == OtaCommit::Settle { STATE_VALID } else { STATE_NEW }) {
        OtaSelection::Selected => return if mode == OtaCommit::Switch { SaveOutcome::Verified } else { SaveOutcome::Unchanged },
        OtaSelection::Unknown => return SaveOutcome::Unconfirmed,
        OtaSelection::Exhausted => return SaveOutcome::Unchanged,
        OtaSelection::Write => (),
    }
    let at = into as u32 * OTA_SECTOR_BYTES;
    if flash.erase(at) { flash.write(at,&entry); }
    for _ in 0..3 {
        if !read_ota_records(flash,&mut records) { continue; }
        let boots = ota_boot_slot(&records);
        if boots == slot { return if mode != OtaCommit::Fresh || records[into as usize] == entry { SaveOutcome::Verified } else { SaveOutcome::Unconfirmed }; }
        return if boots < 0 { SaveOutcome::Unconfirmed } else { SaveOutcome::Unchanged };
    }
    SaveOutcome::Unconfirmed
}
