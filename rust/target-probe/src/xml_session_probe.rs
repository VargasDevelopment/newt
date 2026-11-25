//! Runtime-borrowed RV32 probe source. Backend is supplied by the target caller;
//! this does not supply/validate a target XML parser or target Expat integration.
use newt_core::member_check::Checked;
use newt_core::xml_session::{Handler, ParserBackend, Resources, XmlSaxSession};
use newt_core::{
    zip_container::{InflateScratch, Status},
    zip_window::source::BookSource,
};

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_xml_session_probe<'a>(
    source: &'a mut dyn BookSource,
    entry: &'a Checked,
    decoder: Option<InflateScratch<'a>>,
    backend: &'a mut dyn ParserBackend,
    handler: &'a mut dyn Handler,
    resources: Resources<'a>,
    filter: bool,
    feed_budget: u32,
) -> (Status, u64, bool) {
    let mut session = XmlSaxSession::new(source, decoder, backend);
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
    let count = session.bytes_consumed();
    session.close();
    (status, count, end)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn newt_rust_xml_producer_probe<'a>(
    source: &'a mut dyn BookSource,
    entry: &'a Checked,
    decoder: Option<InflateScratch<'a>>,
    backend: &'a mut dyn newt_core::xml_session::ProducerBackend,
    handler: &'a mut dyn Handler,
    resources: Resources<'a>,
    filter: bool,
    feed_budget: u32,
    policy: &mut dyn newt_core::xml_session::ProcessingPolicy,
) -> (Status, u64, bool) {
    let mut session = XmlSaxSession::with_policy(
        source,
        decoder,
        backend,
        newt_core::xml_session::BorrowedPolicy(policy),
    );
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
    let count = session.bytes_consumed();
    session.close();
    (status, count, end)
}
