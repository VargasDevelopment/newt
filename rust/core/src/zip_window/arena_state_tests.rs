// SPDX-License-Identifier: MIT
use super::{Arena, ArenaState};

#[test]
fn unchanged_bank_resumes_offsets_and_diagnostics_between_loans() {
    let mut bytes = [0; 64];
    let (state, kept) = {
        let mut arena = Arena::new(&mut bytes);
        assert_eq!(arena.alloc(3, 1).unwrap().offset(), 0);
        let kept = arena.copy_nul_ref(Some(b"kept")).unwrap();
        assert_eq!(kept.offset(), 3);
        assert_eq!(arena.alloc(16, 8).unwrap().offset(), 8);
        arena.release(8);
        assert!(arena.alloc(70, 1).is_none());
        (arena.state(), kept)
    };
    let state = {
        let mut arena = Arena::from_state(&mut bytes, state).unwrap();
        assert_eq!(arena.capacity(), 64);
        assert_eq!(arena.used(), 8);
        assert_eq!(arena.high_water(), 24);
        assert_eq!(arena.failed_alloc_size(), 70);
        assert_eq!(arena.c_string(kept), Some(b"kept".as_slice()));
        assert_eq!(arena.alloc(4, 16).unwrap().offset(), 16);
        assert_eq!(arena.used(), 20);
        arena.state()
    };
    let mut arena = Arena::from_state(&mut bytes, state).unwrap();
    assert_eq!(arena.alloc(4, 8).unwrap().offset(), 24);
    assert_eq!(arena.high_water(), 28);
    assert_eq!(arena.failed_alloc_size(), 70);
}

#[test]
fn withdrawn_tail_preserves_historical_high_water_above_prefix_capacity() {
    let mut bytes = [0; 64];
    let state = {
        let mut arena = Arena::new(&mut bytes);
        arena.alloc(64, 1).unwrap();
        arena.release(8);
        arena.take_tail(16, 1).unwrap().fill(0x77);
        assert_eq!(arena.capacity(), 48);
        assert!(arena.alloc(49, 1).is_none());
        arena.state()
    };
    {
        let mut arena = Arena::from_state(&mut bytes[..48], state).unwrap();
        assert_eq!(arena.used(), 8);
        assert_eq!(arena.high_water(), 64);
        assert_eq!(arena.failed_alloc_size(), 49);
        assert_eq!(arena.alloc(40, 1).unwrap().offset(), 8);
        assert!(arena.alloc(1, 1).is_none());
        assert_eq!(arena.high_water(), 64);
        assert_eq!(arena.failed_alloc_size(), 49);
    }
    assert_eq!(bytes[48..], [0x77; 16]);
}

#[test]
fn invalid_extent_or_offsets_return_the_unchanged_state() {
    let mut bytes = [0x55; 32];
    for (capacity, used, high_water) in [(31, 8, 16), (33, 8, 16), (32, 33, 40), (32, 9, 8)] {
        let state = ArenaState {
            capacity,
            used,
            high_water,
            failed_alloc_size: 99,
        };
        let rejected = match Arena::from_state(&mut bytes, state) {
            Err(state) => state,
            Ok(_) => panic!("invalid allocator continuation accepted"),
        };
        assert_eq!(rejected.capacity, capacity);
        assert_eq!(rejected.used, used);
        assert_eq!(rejected.high_water, high_water);
        assert_eq!(rejected.failed_alloc_size, 99);
        assert_eq!(bytes, [0x55; 32]);
    }
}
