extern crate std;
use super::*;
use crate::durable::{BlobRead, DurableWrite, StorageOpen};
use std::{string::String, vec::Vec};

const GOLDEN: [u8; 20] = [
    0x4e, 0x57, 0x51, 0x53, 1, 0, 0, 0, 255, 255, 255, 255, 5, 3, 0, 0, 0x13, 0x6f, 0x0f, 0x31,
];
const RECORD: Record = Record {
    finished: true,
    step: Step::Done,
    from_settings: true,
};

#[test]
fn independent_wire_and_unknown_steps() {
    let mut bytes = [0xa5; BYTES];
    assert!(QuickStartCodec::encode(RECORD, u32::MAX, &mut bytes));
    assert_eq!(bytes, GOLDEN);
    assert_eq!(QuickStartCodec::decode(&GOLDEN), Some(RECORD));
    // Independent wire bytes distinguish the Settings and finished bits.
    for (wire, expected) in [
        (
            [
                0x4e, 0x57, 0x51, 0x53, 1, 0, 0, 0, 17, 0, 0, 0, 3, 1, 0, 0, 0x62, 0xeb, 0x2a, 0x29,
            ],
            Record {
                finished: false,
                step: Step::Wifi,
                from_settings: true,
            },
        ),
        (
            [
                0x4e, 0x57, 0x51, 0x53, 1, 0, 0, 0, 17, 0, 0, 0, 3, 2, 0, 0, 0x17, 0x38, 0x44, 0xea,
            ],
            Record {
                finished: true,
                step: Step::Wifi,
                from_settings: false,
            },
        ),
    ] {
        assert_eq!(QuickStartCodec::decode(&wire), Some(expected));
        QuickStartCodec::encode(expected, 17, &mut bytes);
        assert_eq!(bytes, wire);
    }
    for payload in [6, 255, 0x400, u32::MAX] {
        let mut bytes = GOLDEN;
        bytes[12..16].copy_from_slice(&u32::to_le_bytes(payload));
        let checksum = fnv1a(&bytes[..16]);
        bytes[16..].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(QuickStartCodec::decode(&bytes), None);
    }
    // Finished at Welcome is permitted by the frozen codec, not normalized.
    let arbitrary = Record {
        step: Step::Welcome,
        ..RECORD
    };
    QuickStartCodec::encode(arbitrary, 0, &mut bytes);
    assert_eq!(QuickStartCodec::decode(&bytes), Some(arbitrary));
}

#[test]
fn extent_version_and_damage_stay_distinct() {
    for n in 0..BYTES {
        assert!(!QuickStartCodec::known(&GOLDEN[..n]));
        assert_eq!(QuickStartCodec::decode(&GOLDEN[..n]), None);
    }
    let mut future = GOLDEN;
    future[4] = 2;
    assert!(!QuickStartCodec::known(&future));
    let mut damaged = GOLDEN;
    damaged[19] ^= 1;
    assert!(QuickStartCodec::known(&damaged));
    assert_eq!(QuickStartCodec::decode(&damaged), None);
    assert!(QuickStartCodec::REPLACE_DAMAGED);
}

#[derive(Default)]
struct Memory {
    slots: [Option<Vec<u8>>; 2],
    durable: bool,
    acknowledge: bool,
    acknowledge_on_write: usize,
    written: Vec<(String, Vec<u8>)>,
    writes: usize,
    opens: Vec<(String, bool)>,
    ends: usize,
}
impl Backend for Memory {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        self.opens.push((String::from(namespace), readonly));
        StorageOpen::Opened
    }
    fn end(&mut self) {
        self.ends += 1;
    }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        let Some(bytes) = &self.slots[usize::from(key == "slot1")] else {
            return BlobRead::Missing;
        };
        if bytes.len() <= output.len() {
            output[..bytes.len()].copy_from_slice(bytes);
        }
        BlobRead::Present(bytes.len())
    }
    fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.writes += 1;
        self.written.push((String::from(key), bytes.to_vec()));
        self.slots[usize::from(key == "slot1")] = Some(bytes.to_vec());
        if self.acknowledge || self.writes == self.acknowledge_on_write {
            bytes.len()
        } else {
            0
        }
    }
    fn readback_durable(&self) -> bool {
        self.durable
    }
}

#[test]
fn cold_resume_uses_card_and_loads_readonly() {
    let mut backend = Memory::default();
    backend.slots[0] = Some(GOLDEN.to_vec());
    assert_eq!(
        resume(&mut backend, |_| panic!("history must not override card")),
        Resume {
            show: false,
            at: RECORD
        }
    );
    assert_eq!(backend.opens, [(String::from("newt_quick"), true)]);
    assert_eq!(backend.ends, 1);
    assert_eq!(backend.writes, 0);
    let record = Record {
        finished: false,
        step: Step::Wifi,
        from_settings: true,
    };
    let mut bytes = [0; BYTES];
    QuickStartCodec::encode(record, 4, &mut bytes);
    backend.slots[0] = Some(bytes.to_vec());
    assert_eq!(
        resume(&mut backend, |_| CardHistory::New),
        Resume {
            show: true,
            at: record
        }
    );
}

#[test]
fn unreadable_records_defer_to_history_without_writes() {
    for history in [CardHistory::Unknown, CardHistory::New, CardHistory::RanNewt] {
        let mut backend = Memory::default();
        let result = resume(&mut backend, |backend| {
            assert_eq!(backend.ends, 1);
            assert_eq!(backend.writes, 0);
            history
        });
        assert_eq!(result.show, history == CardHistory::New);
        assert_eq!(result.at, Record::default());
        assert_eq!(backend.writes, 0);
    }
}

#[test]
fn lost_acknowledgement_needs_backend_durability() {
    for durable in [false, true] {
        let mut backend = Memory {
            durable,
            ..Memory::default()
        };
        let result = commit(&mut backend, RECORD);
        assert_eq!(
            result,
            if durable {
                Commit::Saved
            } else {
                Commit::Unconfirmed
            }
        );
        assert_eq!(backend.writes, if durable { 1 } else { 2 });
        assert_eq!(
            backend.opens.iter().filter(|(_, ro)| *ro).count(),
            usize::from(durable)
        );
        assert!(
            backend
                .opens
                .iter()
                .all(|(namespace, _)| namespace == "newt_quick")
        );
        assert_eq!(backend.ends, backend.opens.len());
    }
    let mut backend = Memory {
        acknowledge_on_write: 2,
        ..Memory::default()
    };
    assert_eq!(commit(&mut backend, RECORD), Commit::Saved);
    assert_eq!(backend.writes, 2);
    assert_eq!(backend.written[0], backend.written[1]);
    assert_eq!(backend.opens.len(), 1); // no read-only reconciliation after a confirmed retry
}
