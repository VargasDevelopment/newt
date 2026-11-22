/// Runtime text and caller-owned output keep formatting paths observable to LTO.
#[unsafe(no_mangle)]
pub fn newt_rust_clock_format_probe(
    text: Option<&core::ffi::CStr>,
    raw: u8,
    output: &mut [u8; newt_core::clock_format::CLOCK_TEXT_BYTES],
) -> usize {
    use newt_core::clock_format::*;
    format_clock_text(text, ClockFormat::from_raw_or_default(raw), output)
        .map_or(0, |shown| shown.to_bytes().len())
}
