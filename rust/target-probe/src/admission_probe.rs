//! Runtime-borrowed inputs keep decoder/policy calls live in the target archive.
//! This is code-generation evidence only; no hardware or firmware entry point.
use newt_core::{utf8, zip_record_rules as zip};

#[unsafe(no_mangle)]
pub fn newt_rust_utf8_probe(bytes: Option<&[u8]>, offset: &mut usize, codepoint: &mut u32) -> bool {
    utf8::next(bytes, offset, codepoint)
}

/// Six runtime fields: signature, local flags/method/name, size, central flags.
/// Central method/name are supplied separately to prevent equality folding.
#[unsafe(no_mangle)]
pub fn newt_rust_zip_probe(input: &[u32; 6], central_method: u16, central_name: u16) -> u64 {
    let flags = input[1] as u16;
    let method = input[2] as u16;
    let name = input[3] as u16;
    let rule = zip::zip_central_record_rule(flags, method, name, input[4]);
    let local = zip::zip_local_header_rule(
        input[0],
        flags,
        method,
        name,
        input[5] as u16,
        central_method,
        central_name,
    );
    u64::from(zip::zip_flag_mask(rule))
        | ((zip::zip_member_flag_rule(flags) as u64) << 16)
        | ((rule as u64) << 24)
        | ((local as u64) << 32)
        | zip::EPUB_MAX_EXPANDED_BYTES
}
