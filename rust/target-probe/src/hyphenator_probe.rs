//! Safe Rust ABI runtime probe. No pointer conversion or constant fixture.
use newt_core::hyphenator::Hyphenator;

/// Caller owns borrowed data and mutable state, so repeated calls exercise
/// failed init, reinit and reuse. Link a target caller to retain this entry.
#[inline(never)]
pub fn hyphenator_borrowed_probe<'a>(
    state: &mut Hyphenator<'a>,
    data: Option<&'a [u8]>,
    reinitialize: bool,
    word: &[u8],
    output: &mut [u8],
) -> (bool, bool, bool, usize, u8, usize) {
    let initialized = if reinitialize {
        state.init(data)
    } else {
        false
    };
    (
        initialized,
        state.ready(),
        state.records_valid(),
        state.pattern_count(),
        state.max_pattern_len(),
        state.break_positions(word, output),
    )
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_hyphenator_borrowed<'a>(
    state: &mut Hyphenator<'a>,
    data: Option<&'a [u8]>,
    reinitialize: bool,
    word: &[u8],
    output: &mut [u8],
) -> (bool, bool, bool, usize, u8, usize) {
    hyphenator_borrowed_probe(state, data, reinitialize, word, output)
}
