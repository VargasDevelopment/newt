use crate::expat::Expat;
use newt_core::xml_session::Handler;
use newt_core::{
    xml_input::EntityFilter,
    xml_session::{Resources, XmlStream},
    zip_container::{Entry, Status},
    zip_window::source::BookSource,
};
struct Bytes(&'static [u8]);
impl BookSource for Bytes {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let tail = self.0.get(at as usize..).unwrap_or_default();
        let n = tail.len().min(out.len());
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
struct Sink;
impl Handler for Sink {}
#[test]
fn native_scope_cleanup_success_parse_failure_early_drop_and_create_failure() {
    for (bytes, early, fail) in [
        (b"<x/>".as_slice(), false, false),
        (b"<".as_slice(), false, false),
        (b"<x/>".as_slice(), true, false),
        (b"<x/>".as_slice(), false, true),
    ] {
        let mut source = Bytes(bytes);
        let mut backend = Expat::default();
        backend.fail_create = fail;
        let entry = Entry::raw(bytes.len() as u32);
        let mut buffer = [0; 2048];
        let mut filter = EntityFilter::new();
        {
            let mut stream = XmlStream::new(&mut source, None, &mut backend);
            let status = stream.open(
                &entry,
                Resources {
                    buffer: Some(&mut buffer),
                    filter: Some(&mut filter),
                },
                false,
            );
            assert_eq!(
                status,
                if fail {
                    Status::OutOfMemory
                } else {
                    Status::Ok
                }
            );
            if !early && !fail {
                let mut end = false;
                let mut status = Status::Ok;
                while !end && status == Status::Ok {
                    status = stream.feed_chunk(&mut Sink, Some(&mut end));
                }
                assert_eq!(
                    status,
                    if bytes == b"<" {
                        Status::ParseError
                    } else {
                        Status::Ok
                    }
                );
            }
        }
        assert_eq!(backend.created, usize::from(!fail));
        assert_eq!(backend.created, backend.closed);
    }
}
