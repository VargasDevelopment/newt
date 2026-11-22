//! Rust ABI probes retain real operations on caller-owned state and runtime data.
//! No C ABI, target execution, stack bound, or firmware link is asserted.
use newt_core::xml_input::{self, Attribute, BytePull, EncryptionScan, EntityFilter, TextCapture};
use newt_core::zip_window::arena::{Arena, ByteRange};

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_xml_filter_borrowed(
    filter: &mut EntityFilter,
    source: &mut dyn BytePull,
    output: &mut [u8],
    reset: bool,
) -> i32 {
    if reset {
        filter.reset();
    }
    filter.read(source, output)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_xml_path_borrowed(
    arena: &mut Arena<'_>,
    base: Option<&[u8]>,
    href: Option<&[u8]>,
    fragment: Option<&mut Option<ByteRange>>,
    directory_path: &[u8],
    directory_output: &mut [u8],
) -> (Option<ByteRange>, bool) {
    let path = xml_input::resolve_href(arena, base, href, fragment);
    (path, xml_input::dir_name(directory_path, directory_output))
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_xml_capture_borrowed(
    capture: &mut TextCapture,
    text: &[u8],
    length: i32,
    operation: u8,
) -> (usize, bool, usize) {
    let trimmed_len = match operation {
        0 => {
            capture.begin();
            0
        }
        1 => {
            capture.end();
            0
        }
        2 => {
            capture.add_event(text, length);
            0
        }
        _ => capture.trimmed().len(),
    };
    (capture.len, capture.active, trimmed_len)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_xml_names_borrowed<'a>(
    scan: &mut EncryptionScan,
    element: &'a [u8],
    attributes: &[Attribute<'a>],
    query: &[u8],
    token: &[u8],
    exact: bool,
) -> (Option<&'a [u8]>, &'a [u8], bool, bool) {
    scan.on_start_element(element, attributes);
    let attribute = if exact {
        xml_input::attr_exact(attributes, query)
    } else {
        xml_input::attr_local(attributes, query)
    };
    (
        attribute,
        xml_input::local_name(element),
        xml_input::local_is(element, query),
        xml_input::has_token(attribute, token),
    )
}
