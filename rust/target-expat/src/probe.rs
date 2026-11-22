//! Runtime-borrowed calls using the actual native backend, not an injected mock.
//! Rust ABI only; parent must explicitly retain and call these in its RV32 harness.
use crate::Expat;
use newt_core::{
    package_parsers::{PackageResult, PackageStorage, Workflow, parse_package},
    xml_input::EntityFilter,
    xml_session::{BorrowedPolicy, Handler, ProcessingPolicy, Resources, XmlSaxSession},
    zip_container::{Entry, InflateScratch, Status},
    zip_window::{arena::Arena, source::BookSource},
};

#[unsafe(no_mangle)]
#[inline(never)]
pub fn nrust38_native_xml_probe(
    source: &mut dyn BookSource,
    entry: &Entry,
    decoder: Option<InflateScratch<'_>>,
    handler: &mut dyn Handler,
    resources: Resources<'_>,
    filter: bool,
    feed_budget: u32,
    policy: &mut dyn ProcessingPolicy,
) -> (Status, u64, bool) {
    let mut backend = Expat::default();
    let mut session =
        XmlSaxSession::with_policy(source, decoder, &mut backend, BorrowedPolicy(policy));
    let mut status = session.open(entry, handler, resources, filter);
    let mut end = false;
    if status == Status::Ok {
        for _ in 0..feed_budget {
            status = session.feed_chunk(Some(&mut end));
            if status != Status::Ok || end {
                break;
            }
        }
    }
    let consumed = session.bytes_consumed();
    session.close();
    (status, consumed, end)
}

/// Runs the actual Core two-pass package parser with caller-owned resources.
/// This runs package parsing; the separate admission root checks the full envelope.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn nrust38_native_package_probe(
    source: &mut dyn BookSource,
    entry: &Entry,
    decoder: Option<InflateScratch<'_>>,
    buffer: Option<&mut [u8; 2048]>,
    filter: Option<&mut EntityFilter>,
    dir: &[u8],
    arena: &mut Arena<'_>,
    storage: PackageStorage<'_>,
    out: &mut PackageResult,
) -> Status {
    let mut backend = Expat::default();
    let mut workflow = Workflow {
                policy: None,
        source,
        backend: &mut backend,
        decoder,
        buffer,
        filter,
    };
    parse_package(&mut workflow, entry, dir, arena, storage, out)
}

/// Actual canonical admission with this native backend and caller-owned storage.
/// The returned arena usage exposes the real unconditional reset boundary.
/// No processing-policy forwarding or resolved target SDK link is claimed here.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn nrust38_native_admission_probe(
    source: &mut dyn BookSource,
    catalog: &mut Arena<'_>,
    parse: &mut Arena<'_>,
    scan: &mut [u8; 4099],
    sequential: &mut [u8; 1024],
    entries: &mut [Entry],
    manifest: &mut [newt_core::package_parsers::ManifestItem],
    spine_hashes: &mut [u32],
    spine: &mut [u16],
    decoder: Option<InflateScratch<'_>>,
    chunk: Option<&mut [u8; 2048]>,
    xml_buffer: Option<&mut [u8; 2048]>,
    filter: Option<&mut EntityFilter>,
) -> (Status, usize, usize) {
    let mut backend = Expat::default();
    let mut resources = newt_core::canonical::Resources {
                policy: None,
        scan,
        sequential,
        entries,
        manifest,
        spine_hashes,
        spine,
        backend: &mut backend,
        decoder,
        chunk,
        xml_buffer,
        filter,
    };
    let status = newt_core::admission::admit(source, catalog, parse, &mut resources, None, None);
    (status, catalog.used(), parse.used())
}
