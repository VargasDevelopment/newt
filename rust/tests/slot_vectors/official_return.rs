//! Cold sidecar bytes supply eligibility, never confirmation. The real
//! installer must reject a missing/mismatched grant before its first erase.
use super::*;
use newt_core::{official_return::{ReturnAuthorization, RECEIPT_BYTES},
    update_card::slot_installer::{Source, identity::{NewtAppIdentity, read_newt_identity}},
    update_manifest::{UpdateManifest, parse_update_manifest}, update_online::TestBuilds, wire::{crc32, put32}};

impl Source for Card {
    fn size(&self) -> u32 { self.data.len() as u32 }
    fn read(&mut self, at: u32, out: &mut [u8]) -> bool {
        let Some(bytes) = self.data.get(at as usize..at as usize + out.len()) else { return false; };
        out.copy_from_slice(bytes); true
    }
}

#[test]
fn only_a_confirmed_matching_return_can_erase() {
    let fixtures = rows("official");
    let current = bytes(&fixtures[0][2]);
    let candidate = bytes(&fixtures[1][2]);
    let mut target = UpdateManifest::default();
    let manifest = bytes(&fixtures[1][3]);
    assert!(parse_update_manifest(Some(&manifest), manifest.len(), &mut target));
    let mut source = NewtAppIdentity::default();
    assert!(read_newt_identity(current[..512].try_into().unwrap(), &mut source));
    // These are the only bytes kept across the cold restart. The real Check
    // issuer and atomic card writer are exercised by the online/preview tests.
    let receipt = {
        let mut bytes = [0; RECEIPT_BYTES]; bytes[..4].copy_from_slice(b"NWOR"); put32(&mut bytes, 4, 1);
        bytes[8..32].copy_from_slice(&source.version); bytes[32..48].copy_from_slice(&source.build);
        bytes[48..80].copy_from_slice(&target.sha256); let crc = crc32(&bytes[..80]); put32(&mut bytes, 80, crc);
        bytes
    };
    let mut mismatches = Mismatches::default();
    for case in ["confirmed", "missing", "corrupt", "not-confirmed", "foreign-source", "changed-target", "other-slot"] {
        let mut saved = receipt.to_vec();
        if case == "missing" { saved.clear(); }
        if case == "corrupt" { saved[32] ^= 1; }
        let grant = ReturnAuthorization::restore(&saved, &source, &target, TestBuilds::Off).ok();
        let permission = if case == "not-confirmed" { None } else { grant.map(|grant| grant.confirm()) };
        let running = OtaRecord { seq: 1, label: [255; 20], state: STATE_VALID, crc: ota_seq_crc(1) };
        let mut old = vec![0x5a; 2 * SECTOR_BYTES];
        let other = rows("head").into_iter().find(|row| row[3] == "other").unwrap();
        old[..512].copy_from_slice(&bytes(&other[2])[..512]);
        let mut flash = Flash { slots: [vec![0xff; 2 * SECTOR_BYTES], old.clone()],
            ota: Otadata([running, ERASED_RECORD]), total: 0, erases: 0 };
        flash.slots[0][..current.len()].copy_from_slice(&current);
        if case == "foreign-source" { flash.slots[0][328] ^= 1; }
        let data = if case == "changed-target" { bytes(&fixtures[2][2]) } else { candidate.clone() };
        let mut manifest = target;
        if case == "changed-target" {
            let encoded = bytes(&fixtures[2][3]);
            assert!(parse_update_manifest(Some(&encoded), encoded.len(), &mut manifest));
        }
        let mut card = Card { data: data.clone(), at: 0, listed: false };
        let consent = InstallConsent { confirmed: case != "other-slot", official_return: permission, ..InstallConsent::default() };
        let issue = SlotInstaller::new(&mut flash, &mut card, &mut Sha256::default(),
            &mut [0; SECTOR_BYTES], &mut [0; SECTOR_BYTES])
            .install(&manifest, Some(c"0.5.1-dev.20261125.1"), consent);
        let expected = if case == "confirmed" { InstallIssue::None }
            else if case == "other-slot" { InstallIssue::Consent } else { InstallIssue::CheckAgain };
        mismatches.check("official return", case, issue, expected);
        if expected != InstallIssue::None {
            mismatches.check("official return", "no erase", flash.erases, 0);
            mismatches.check("official return", "slot unchanged", flash.slots[1] == old, true);
        } else {
            mismatches.check("official return", "target image", &flash.slots[1][..data.len()], data.as_slice());
        }
    }
    mismatches.finish();
}
