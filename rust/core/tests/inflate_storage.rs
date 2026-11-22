use newt_core::inflate::{BitBuffer, State, Status, HAS_MORE_INPUT, NON_WRAPPING};
use core::mem::{align_of, size_of};

const BYTES: usize = size_of::<State>();
#[repr(align(16))]
struct Region([u8; BYTES + 32]);

fn placed<R>(mode: BitBuffer, work: impl FnOnce(&mut State) -> R) -> R {
    let mut region = Region([0xa5; BYTES + 32]);
    let bytes = &mut region.0[16..16 + BYTES];
    assert_eq!(bytes.as_ptr().align_offset(align_of::<State>()), 0);
    assert!(State::initialize_storage(bytes, mode));
    // SAFETY: exact extent and checked alignment; initialize_storage wrote all
    // fields' valid native representations. The exclusive loan ends before
    // the region or its guards are inspected or dropped.
    let result = work(unsafe { &mut *bytes.as_mut_ptr().cast::<State>() });
    assert_eq!(&region.0[..16], &[0xa5; 16]);
    assert_eq!(&region.0[16 + BYTES..], &[0xa5; 16]);
    result
}

#[test]
fn wrong_extent_is_rejected_without_mutation() {
    for size in [0, BYTES - 1, BYTES + 1] {
        let mut bytes = vec![0xa5; size];
        assert!(!State::initialize_storage(&mut bytes, BitBuffer::Bits64));
        assert!(bytes.iter().all(|byte| *byte == 0xa5));
    }
}

#[test]
fn placed_state_matches_fresh_reset_failure_and_suspended_output() {
    for mode in [BitBuffer::Bits32, BitBuffer::Bits64] {
        placed(mode, |state| {
            let mut fresh = State::new(mode);
            let mut actual = [0; 16];
            let mut expected = [0; 16];
            for (input, capacity, flags) in [
                (&[][..], 3, 0),
                (&[1, 1, 0, 254, 255, 65][..], 0, NON_WRAPPING),
                (&[65][..], 16, NON_WRAPPING),
                (&[][..], 16, NON_WRAPPING),
            ] {
                assert_eq!(state.decompress(input, &mut actual, 0, capacity, flags),
                    fresh.decompress(input, &mut expected, 0, capacity, flags));
                assert_eq!(actual, expected);
            }
            assert_eq!(actual[0], b'A');
            state.reset(); fresh.reset();
            for input in [&[7][..], &[3, 0][..]] {
                let result = state.decompress(input, &mut actual, 0, 16, NON_WRAPPING);
                assert_eq!(result, fresh.decompress(input, &mut expected, 0, 16, NON_WRAPPING));
                assert_eq!(result.status, Status::Failed);
            }
            state.reset(); fresh.reset();
            let result = state.decompress(&[3, 0], &mut actual, 0, 16, NON_WRAPPING);
            assert_eq!(result, fresh.decompress(&[3, 0], &mut expected, 0, 16, NON_WRAPPING));
            assert_eq!(result.status, Status::Done);
        });
    }
}

#[test]
fn placed_state_preserves_huffman_suspension_in_both_bit_modes() {
    for mode in [BitBuffer::Bits32, BitBuffer::Bits64] {
        placed(mode, |state| {
            let mut fresh = State::new(mode);
            let mut actual = [0; 16];
            let mut expected = [0; 16];
            let mut first = [0; 36];
            first[..4].copy_from_slice(&[5, 0, 0, 4]);
            for input in [&first[..], &[0; 4][..]] {
                let result = state.decompress(input, &mut actual, 0, 16, NON_WRAPPING | HAS_MORE_INPUT);
                assert_eq!(result, fresh.decompress(input, &mut expected, 0, 16, NON_WRAPPING | HAS_MORE_INPUT));
                assert_eq!(actual, expected);
            }
        });
    }
}
