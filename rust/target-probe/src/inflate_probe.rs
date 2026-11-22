use newt_core::inflate::{Progress, State};

// Runtime inputs, state and history are borrowed. No fixture or large local
// constructor can stand in for the decoder in this target-retention probe.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_inflate_probe(
    state: &mut State,
    dictionary: &mut [u8; 32768],
    input: &[u8],
    offset: usize,
    flags: u32,
) -> Progress {
    state.decompress(
        input,
        dictionary,
        offset,
        32768usize.saturating_sub(offset),
        flags,
    )
}
