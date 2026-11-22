use super::expat;
use newt_core::xml_session::*;
use newt_core::{
    zip_container::{Entry, Status},
    zip_window::source::BookSource,
};
struct Source<'a>(&'a [u8], usize);
impl BookSource for Source<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Some(tail) = self.0.get(at as usize..) else {
            return -1;
        };
        let n = tail.len().min(self.1).min(out.len());
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
#[derive(Default)]
struct Sink(Vec<String>);
impl Handler for Sink {
    fn start_element(&mut self, name: &[u8], attrs: &mut dyn Attributes) {
        let mut event = format!("S {}", String::from_utf8_lossy(name));
        while let Some((n, v)) = attrs.next() {
            event += &format!(
                " {}={}",
                String::from_utf8_lossy(n),
                String::from_utf8_lossy(v)
            );
        }
        self.0.push(event);
    }
    fn end_element(&mut self, name: &[u8]) {
        self.0.push(format!("E {}", String::from_utf8_lossy(name)));
    }
    fn text(&mut self, text: &[u8]) {
        self.0.push(format!("T {}", String::from_utf8_lossy(text)));
    }
}
#[test]
fn real_xml_valid_malformed_strict_entities_and_filtering() {
    let cases: &[(&[u8], bool, Status, &[&str])] = &[
        (
            b"<r a='v'>x</r>",
            false,
            Status::Ok,
            &["S r a=v", "T x", "E r"],
        ),
        (b"<r>&amp;</r>", false, Status::Ok, &["S r", "T &", "E r"]),
        (b"<r>&nbsp;</r>", false, Status::ParseError, &["S r"]),
        (
            b"<r>&nbsp;</r>",
            true,
            Status::Ok,
            &["S r", "T \u{a0}", "E r"],
        ),
        (b"<r></s>", false, Status::ParseError, &["S r"]),
        (b"", false, Status::ParseError, &[]),
        (b"<r>", false, Status::ParseError, &["S r"]),
    ];
    for &(xml, filtering, expected, events) in cases {
        for max in [1, 2, 2048] {
            let mut source = Source(xml, max);
            let entry = Entry::raw(xml.len() as u32);
            let mut parser = expat::Expat::default();
            let mut handler = Sink::default();
            let mut buffer = [0; 2048];
            let mut filter = newt_core::xml_input::EntityFilter::new();
            assert_eq!(
                parse_entry(
                    &mut source,
                    &entry,
                    None,
                    &mut parser,
                    &mut handler,
                    Resources {
                        buffer: Some(&mut buffer),
                        filter: Some(&mut filter)
                    },
                    filtering
                ),
                expected
            );
            assert_eq!(handler.0, events, "max={max}, xml={xml:?}");
        }
    }
}

struct NeverCancel;
impl ProcessingPolicy for NeverCancel {
    fn cancelled(&mut self) -> bool {
        false
    }
}
#[test]
fn producer_typed_resources_and_scoped_real_expat_create_oom() {
    for missing in 0..4 {
        let mut source = Source(b"<r/>", 2048);
        let entry = Entry::raw(4);
        let mut parser = expat::Expat::default();
        parser.fail_create = missing == 2;
        let mut handler = Sink::default();
        let mut buffer = [0; 2048];
        let mut filter = newt_core::xml_input::EntityFilter::new();
        let mut policy = NeverCancel;
        let result = parse_entry_with_policy(
            &mut source,
            &entry,
            None,
            &mut parser,
            &mut handler,
            Resources {
                buffer: if missing == 0 {
                    None
                } else {
                    Some(&mut buffer)
                },
                filter: if missing == 1 {
                    None
                } else {
                    Some(&mut filter)
                },
            },
            true,
            BorrowedPolicy(&mut policy),
        );
        assert_eq!(
            result,
            if missing < 3 {
                Status::OutOfMemory
            } else {
                Status::Ok
            }
        );
        if missing < 3 {
            assert!(handler.0.is_empty());
        }
    }
}
#[test]
fn producer_missing_decoder_precedes_other_admission() {
    let mut local = [0; 30];
    local[..4].copy_from_slice(b"PK\x03\x04");
    let mut source = Source(&local, 2048);
    let entry = Entry {
        method: 8,
        compressed_size: 2,
        uncompressed_size: 1,
        ..Default::default()
    };
    let mut backend = expat::Expat::default();
    let mut handler = Sink::default();
    let mut policy = NeverCancel;
    assert_eq!(
        parse_entry_with_policy(
            &mut source,
            &entry,
            None,
            &mut backend,
            &mut handler,
            Resources {
                buffer: None,
                filter: None
            },
            true,
            BorrowedPolicy(&mut policy)
        ),
        Status::OutOfMemory
    );
    assert!(handler.0.is_empty());
}
