//! Runtime-input retention root; parent exports and calls this from its actual
//! RV32 borrowed target. This is not a target/native-backend acceptance result.
use crate::*;
use newt_core::{
    package_parsers::TocEntry,
    xml_session::ParserBackend,
    zip_container::Status,
    zip_window::{arena::Arena, source::BookSource},
};
use newt_phased_sink::{Frame, Target};
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn turn<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    reader: &mut PhasedReader<'_, '_>,
    source: &mut S,
    spine: u32,
    page: u32,
    frame: &mut Frame,
    metadata: &mut Arena<'_>,
    parse: &mut Arena<'_>,
    r: &mut canonical::Resources<'_, B>,
    toc: &mut [TocEntry],
    sr: &mut StyleResources<'_, '_>,
    sources: &mut styles::Sources<'_>,
    slot: &mut styles::Slot<'_>,
    lr: &mut LayoutResources<'_>,
    lease: Option<&Lease>,
) -> Status {
    reader.show(
        source,
        spine,
        page,
        Target::Page,
        frame,
        metadata,
        parse,
        r,
        toc,
        sr,
        sources,
        slot,
        lr,
        lease,
    )
}
