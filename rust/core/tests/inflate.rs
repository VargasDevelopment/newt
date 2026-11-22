use newt_core::inflate::*;
#[test]
fn suspended_slow_huffman_does_not_enter_fast_decoder() {
    for mode in [BitBuffer::Bits32, BitBuffer::Bits64] {
        let mut state = State::new(mode);
        let mut output = [0; 16];
        let mut first = [0; 36];
        first[..4].copy_from_slice(&[5, 0, 0, 4]);
        let flags = NON_WRAPPING | HAS_MORE_INPUT;
        assert_eq!(
            state.decompress(&first, &mut output, 0, 16, flags).status,
            Status::NeedsMoreInput
        );
        let resumed = state.decompress(&[0; 4], &mut output, 0, 16, flags);
        assert_eq!(
            (resumed.status, resumed.consumed, resumed.produced),
            (Status::HasMoreOutput, 0, 16)
        );
        assert_eq!(output, [0; 16]);
    }
}

#[test]
fn suspended_match_retains_history_cursor_across_flag_change() {
    for mode in [BitBuffer::Bits32, BitBuffer::Bits64] {
        let mut state = State::new(mode);
        let mut output = [0; 33026];
        output[32766] = b'A';
        output[32768] = b'B';
        let first = state.decompress(&[3, 2, 0], &mut output, 32767, 1, 0);
        assert_eq!(
            (first.status, first.consumed, first.produced),
            (Status::HasMoreOutput, 2, 1)
        );
        let resumed = state.decompress(&[0], &mut output, 0, 2, NON_WRAPPING);
        assert_eq!(
            (resumed.status, resumed.consumed, resumed.produced),
            (Status::Done, 1, 2)
        );
        assert_eq!(&output[..2], b"AB");
    }
}

#[test]
fn fresh_bad_parameter_preserves_zero_initialized_adler() {
    for mode in [BitBuffer::Bits32, BitBuffer::Bits64] {
        let mut state = State::new(mode);
        let mut output = [0; 3];
        let result = state.decompress(&[], &mut output, 0, 3, 0);
        assert_eq!(
            (
                result.status,
                result.consumed,
                result.produced,
                result.adler32
            ),
            (Status::BadParam, 0, 0, 0)
        );
    }
}

#[test]
fn hand_stored_abc_and_repeat() {
    for mode in [BitBuffer::Bits32, BitBuffer::Bits64] {
        let mut s = State::new(mode);
        let mut out = [0; 32768];
        let r = s.decompress(
            &[1, 3, 0, 252, 255, 97, 98, 99],
            &mut out,
            0,
            32768,
            COMPUTE_ADLER32,
        );
        assert_eq!(
            r,
            Progress {
                status: Status::Done,
                consumed: 8,
                produced: 3,
                adler32: 0x024d0127
            }
        );
        assert_eq!(&out[..3], b"abc");
        assert_eq!(
            s.decompress(&[], &mut out, 3, 32765, COMPUTE_ADLER32)
                .status,
            Status::Done
        );
    }
}
#[test]
fn hand_fixed_empty_raw_and_zlib() {
    let mut s = State::new(BitBuffer::Bits32);
    let mut out = [0; 32768];
    assert_eq!(
        s.decompress(&[3, 0], &mut out, 0, 32768, 0).status,
        Status::Done
    );
    s.reset();
    let r = s.decompress(
        &[0x78, 0x9c, 3, 0, 0, 0, 0, 1],
        &mut out,
        0,
        32768,
        PARSE_ZLIB_HEADER,
    );
    assert_eq!(r.status, Status::Done);
    assert_eq!(r.adler32, 1);
    assert_eq!(r.consumed, 8);
}
#[test]
fn resumable_starvation_is_not_terminal() {
    let mut s = State::new(BitBuffer::Bits32);
    let mut out = [0; 32768];
    assert_eq!(
        s.decompress(&[], &mut out, 0, 32768, 0).status,
        Status::CannotMakeProgress
    );
    assert_eq!(
        s.decompress(&[], &mut out, 0, 32768, HAS_MORE_INPUT).status,
        Status::NeedsMoreInput
    );
    assert_eq!(
        s.decompress(&[3, 0], &mut out, 0, 32768, 0).status,
        Status::Done
    );
}
#[test]
fn mismatch_repeats_as_actual_c_implementation() {
    let mut s = State::new(BitBuffer::Bits64);
    let mut out = [0; 32768];
    let r = s.decompress(
        &[0x78, 0x9c, 3, 0, 0, 0, 0, 2],
        &mut out,
        0,
        32768,
        PARSE_ZLIB_HEADER,
    );
    assert_eq!(r.status, Status::AdlerMismatch);
    assert_eq!(
        s.decompress(&[], &mut out, 0, 32768, PARSE_ZLIB_HEADER)
            .status,
        Status::AdlerMismatch
    );
    assert_eq!(
        s.decompress(&[], &mut out, 0, 32768, 0).status,
        Status::Done
    );
}
#[test]
fn stored_output_pending_and_terminal_failure() {
    let mut s = State::new(BitBuffer::Bits32);
    let mut out = [0; 16];
    let r = s.decompress(&[1, 1, 0, 254, 255, 65], &mut out, 0, 0, NON_WRAPPING);
    assert_eq!(r.status, Status::HasMoreOutput);
    assert_eq!(r.consumed, 5);
    let r = s.decompress(&[65], &mut out, 0, 16, NON_WRAPPING);
    assert_eq!(r.status, Status::Done);
    assert_eq!(&out[..1], b"A");
    s.reset();
    assert_eq!(
        s.decompress(&[7], &mut out, 0, 16, NON_WRAPPING).status,
        Status::Failed
    );
    assert_eq!(
        s.decompress(&[3, 0], &mut out, 0, 16, NON_WRAPPING).status,
        Status::Failed
    );
}
