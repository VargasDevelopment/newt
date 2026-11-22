// SPDX-License-Identifier: MIT
//! Authored state boundaries and independent Python struct/zlib NWPA bytes.
extern crate std;
use super::app::Action;
use super::durable::{Backend, BlobRead, DurableWrite, StorageOpen};
use super::pause::*;
use super::pause_store::{Store, StoreError};
use std::vec::Vec;

#[test]
fn selection_avoids_repetition_only_when_an_alternative_exists() {
    for (mask, last, random, wanted) in [
        (0, Prompt::BREATH, 3, Prompt::NONE),
        (8, Prompt::NONE, 0, Prompt::NONE),
        (1, Prompt::BREATH, u32::MAX, Prompt::BREATH),
        (7, Prompt::BREATH, 0, Prompt::MOVE),
        (7, Prompt::BREATH, 1, Prompt::WATER),
        (6, Prompt::WATER, 31, Prompt::MOVE),
        (7, Prompt(37), u32::MAX, Prompt::BREATH),
    ] {
        assert_eq!(choose(mask, last, random), wanted, "candidate selection");
    }
    let mut state = State {
        last_pick: Prompt(37),
        ..State::default()
    };
    assert!(state.open(0, 0));
    assert_eq!(
        (state.selected_prompt, state.last_pick),
        (Prompt::NONE, Prompt(37))
    );
    let before = state;
    assert!(!state.open(8, 1));
    assert_eq!(state, before, "invalid masks do not publish state");
}

#[test]
fn draft_navigation_cancel_and_save_preserve_committed_choices() {
    let mut state = State::default();
    assert!(state.open(7, 0));
    assert_eq!(state.dispatch(Action::Activate, 0), Intent::None);
    assert_eq!(state.dispatch(Action::Activate, 0), Intent::None);
    assert_eq!((state.enabled_mask, state.draft_mask), (7, 6));
    assert_eq!(state.dispatch(Action::Back, 0), Intent::None);
    assert_eq!((state.view, state.draft_mask), (View::Experience, 7));
    state.dispatch(Action::Activate, 0);
    state.settings_focus = 99;
    state.dispatch(Action::Previous, 0);
    assert_eq!(state.settings_focus, 3);
    assert_eq!(state.dispatch(Action::Activate, 0), Intent::Save);
    state.draft_mask = 2;
    state.finish_save(false, 0);
    assert_eq!(
        (
            state.enabled_mask,
            state.draft_mask,
            state.view,
            state.save_unconfirmed
        ),
        (7, 2, View::Settings, true)
    );
    state.finish_save(true, 0);
    assert_eq!(
        (
            state.enabled_mask,
            state.selected_prompt,
            state.last_pick,
            state.save_unconfirmed
        ),
        (2, Prompt::MOVE, Prompt::MOVE, false)
    );
    assert_eq!(state.dispatch(Action::Back, 0), Intent::Home);
    assert_eq!(state.dispatch(Action::Sleep, 0), Intent::Sleep);
}

#[test]
fn resume_discards_drafts_and_does_not_pick_a_disabled_prompt() {
    let mut state = State {
        draft_mask: 0,
        settings_focus: 3,
        save_unconfirmed: true,
        ..State::default()
    };
    assert!(state.resume(2, Prompt::BREATH, false));
    assert_eq!(
        state,
        State {
            view: View::Settings,
            enabled_mask: 2,
            draft_mask: 2,
            ..State::default()
        }
    );
    assert!(state.resume(2, Prompt::MOVE, true));
    assert_eq!(
        (state.selected_prompt, state.last_pick),
        (Prompt::MOVE, Prompt::MOVE)
    );
    let before = state;
    assert!(!state.resume(8, Prompt::NONE, true));
    assert_eq!(state, before);
}

#[test]
fn nwpa_bytes_and_unsupported_precedence_are_independent() {
    let value = Preferences {
        sequence: 17,
        enabled_mask: 1,
    };
    let mut bytes = [0; 20];
    assert!(value.encode_into(&mut bytes));
    assert_eq!(bytes, ONE, "literal struct/zlib vector");
    assert_eq!(Preferences::decode(&ONE), Ok(value));
    let mut short = [0xa5; 19];
    assert!(!value.encode_into(&mut short));
    assert_eq!(short, [0xa5; 19]);
    let mut bad = [0xa5; 20];
    assert!(
        !Preferences {
            enabled_mask: 8,
            ..value
        }
        .encode_into(&mut bad)
    );
    assert_eq!(bad, [0xa5; 20]);
    let mut future = ONE;
    future[4] = 2;
    assert_eq!(
        Preferences::decode(&future[..8]),
        Err(DecodeError::Unsupported)
    );
    assert_eq!(Preferences::decode(&future), Err(DecodeError::Unsupported));
    future[0] = 0;
    assert_eq!(Preferences::decode(&future), Err(DecodeError::Corrupt));
    for n in 0..20 {
        assert_eq!(Preferences::decode(&ONE[..n]), Err(DecodeError::Corrupt));
    }
}

#[derive(Default)]
struct Memory {
    present: bool,
    records: [Option<Vec<u8>>; 2],
    reads: Vec<(usize, usize)>,
    ends: u32,
    puts: u32,
    readonly: bool,
    fail_read: Option<usize>,
    lose_ack: bool,
}
impl Backend for Memory {
    fn begin(&mut self, name: &str, readonly: bool) -> StorageOpen {
        assert_eq!(name, "newt_pause");
        self.readonly = readonly;
        if readonly && !self.present {
            StorageOpen::Missing
        } else {
            self.present = true;
            StorageOpen::Opened
        }
    }
    fn end(&mut self) {
        self.ends += 1;
        self.readonly = true;
    }
    fn read_blob(&mut self, key: &str, out: &mut [u8]) -> BlobRead {
        let index = if key == "slot0" {
            0
        } else {
            assert_eq!(key, "slot1");
            1
        };
        self.reads.push((index, out.len()));
        if self.fail_read == Some(index) {
            return BlobRead::Failed;
        }
        match &self.records[index] {
            None => BlobRead::Missing,
            Some(bytes) => {
                if bytes.len() == out.len() {
                    out.copy_from_slice(bytes);
                }
                BlobRead::Present(bytes.len())
            }
        }
    }
    fn put_bytes(&mut self, _permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        assert!(!self.readonly);
        self.puts += 1;
        let index = if key == "slot0" {
            0
        } else {
            assert_eq!(key, "slot1");
            1
        };
        self.records[index] = Some(bytes.to_vec());
        if self.lose_ack { 0 } else { bytes.len() }
    }
    fn readback_durable(&self) -> bool {
        true
    }
}

#[test]
fn strict_peers_and_failure_precedence_do_not_recover_a_corrupt_choice() {
    let mut bad = ONE;
    bad[19] ^= 1;
    let mut memory = Memory {
        present: true,
        records: [Some(ONE.to_vec()), Some(bad.to_vec())],
        ..Memory::default()
    };
    assert_eq!(Store::new(&mut memory).load(), Err(StoreError::Corrupt));
    assert_eq!(memory.reads, [(0, 20), (1, 20)]);
    assert_eq!(memory.ends, 1);
    let mut future = ONE[..8].to_vec();
    future[4] = 2;
    memory.records = [Some(bad.to_vec()), Some(future)];
    memory.reads.clear();
    assert_eq!(Store::new(&mut memory).load(), Err(StoreError::Unsupported));
    assert_eq!(
        memory.reads,
        [(0, 20), (1, 20), (1, 8)],
        "mismatched NVS length is reread exactly"
    );
    memory.fail_read = Some(0);
    memory.reads.clear();
    assert_eq!(Store::new(&mut memory).load(), Err(StoreError::Failed));
    assert_eq!(
        memory.reads,
        [(0, 20), (1, 20), (1, 8)],
        "both peers observed before failure selection"
    );
}

#[test]
fn generation_order_no_write_and_wrap_keep_the_protected_peer() {
    let mut memory = Memory {
        present: true,
        records: [Some(ONE.to_vec()), Some(SIX.to_vec())],
        ..Memory::default()
    };
    let mut value = Preferences {
        sequence: 99,
        enabled_mask: 6,
    };
    assert_eq!(Store::new(&mut memory).save(&mut value), Ok(()));
    assert_eq!(
        value,
        Preferences {
            sequence: 18,
            enabled_mask: 6
        }
    );
    assert_eq!(memory.puts, 0);
    memory.records = [Some(MAX.to_vec()), None];
    value = Preferences {
        sequence: 55,
        enabled_mask: 6,
    };
    assert_eq!(Store::new(&mut memory).save(&mut value), Ok(()));
    assert_eq!(
        value,
        Preferences {
            sequence: 0,
            enabled_mask: 6
        }
    );
    assert_eq!(memory.records[0].as_deref(), Some(MAX.as_slice()));
    assert_eq!(Store::new(&mut memory).load(), Ok(value));
}

#[test]
fn lost_ack_retains_runtime_while_a_new_complete_generation_can_survive() {
    let mut memory = Memory {
        present: true,
        records: [Some(ONE.to_vec()), None],
        lose_ack: true,
        ..Memory::default()
    };
    let mut proposed = Preferences {
        sequence: 17,
        enabled_mask: 6,
    };
    let before = proposed;
    assert_eq!(
        Store::new(&mut memory).save(&mut proposed),
        Err(StoreError::Failed)
    );
    assert_eq!(proposed, before);
    assert_eq!(memory.records[0].as_deref(), Some(ONE.as_slice()));
    assert_eq!(
        Store::new(&mut memory).load(),
        Ok(Preferences {
            sequence: 18,
            enabled_mask: 6
        })
    );
}
const ONE: [u8; 20] = [
    78, 87, 80, 65, 1, 0, 0, 0, 17, 0, 0, 0, 1, 0, 0, 0, 32, 57, 199, 35,
];
const SIX: [u8; 20] = [
    78, 87, 80, 65, 1, 0, 0, 0, 18, 0, 0, 0, 6, 0, 0, 0, 122, 6, 159, 48,
];
const MAX: [u8; 20] = [
    78, 87, 80, 65, 1, 0, 0, 0, 255, 255, 255, 255, 1, 0, 0, 0, 3, 40, 11, 9,
];
