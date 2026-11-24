//! Host-only empty/present stylesheet distinction from frozen lendStyles.
use super::*;
fn input(id: u32) -> Vec<u8> {
    let mut bytes = b"<html><body>".to_vec();
    let wrapped = id % 2 != 0;
    if wrapped { bytes.extend_from_slice(b"<p>"); }
    let depth = if wrapped { 13 } else { 14 };
    for _ in 0..depth { bytes.extend_from_slice(b"<span>"); }
    bytes.extend_from_slice(b"<b>A</b>");
    for _ in 0..depth { bytes.extend_from_slice(b"</span>"); }
    if wrapped { bytes.extend_from_slice(b"</p>"); }
    bytes.extend_from_slice(b"</body></html>");
    bytes
}
fn empty_case(id: u32, bytes: &[u8]) -> Output {
    let t = Trace::new(100 + id);
    let cancel = Cancellation::new(Hooks { context: &t, stop: Some(stop), yield_now: Some(yield_now), pause: Some(pause) });
    let mut source = Input { t: &t, bytes };
    let mut shared = [0; BANK_BYTES];
    let mut native = vec![0xa5; newt_phased_sink::FRAME_BYTES];
    let signal = Signals::default();
    let mut loan = Loan { bytes: &mut native, signal: &signal, returns: 0 };
    let mut storage = Storage::new(&mut shared);
    let mut selections = [Selection::default(), Selection::default()];
    selections[0].entry = Entry::raw(bytes.len() as u32);
    let mut reader = PhasedReader::new(&mut selections, None, PageLayout::PORTRAIT);
    reader.source_bytes = bytes.len() as u64;
    reader.info.page_count = 1;
    reader.info.total_chars = 1;
    let mut hash = Hash;
    let mut route = Route { settings: newt_core::preferences::ViewSettings { orientation: newt_core::preferences::Orientation::Portrait, ..Default::default() }, digest: &[0; 32], info: reader.info, hash: &mut hash };
    let mut cache = StorageOut(&t);
    let mut parser = Native { t: &t, xml: Expat::default() };
    let mut breaker = linebreak::NativeBreak;
    let status = storage.with_producer(&mut loan, &cancel, &mut cache, &mut source, reader.info.page_count,None, |owner| owner.with_buffered(|phase, mut attempt| { attempt.with_writer(|a| {
        phase.defer(0)?;
        if id >= 2 {
            phase.edit_slot(|slot| {
                slot.rules[0] = newt_core::css::CssRule::default();
                slot.rules[0].elem_hash = u32::MAX;
                slot.rules[0].decl.size_pct = 200;
                slot.count = 1;
            })?;
        }
        a.state.name[0] = b'c';
        assert!(a.writer.cache.begin(a.store, &a.state.name[..69], 123, &a.memory));
        let status = a.finish_current(&reader, phase, &mut parser, &mut breaker, &mut route);
        t.out.borrow_mut().pages = a.writer.cache.page_count();
        assert_eq!(t.out.borrow().live, 0);
        Ok(status)
    })? })).unwrap();
    t.out.borrow_mut().status = status as u32;
    t.out.borrow_mut().interrupted = cancel.interrupted() as u32;
    t.out.into_inner()
}
fn fields(o: &Output) -> [u32; 12] {
    [o.status,o.pages,o.pauses,o.feeds,o.creates,o.live,o.interrupted,o.ends,o.polls,o.yields,o.bytes,o.events]
}
#[test]
fn against_original() {
    for id in 0..4 {
        let bytes = input(id);
        let (cpp, _) = captured_output(2, id, &bytes);
        let actual = empty_case(id, &bytes);
        assert_eq!(fields(&actual), fields(&cpp), "fields case{id}");
        assert!(actual.bytes > 28);
        assert_eq!(actual.data[28], if id < 2 { 1 } else { 0 }, "source-derived style case{id}");
        assert_eq!(&actual.data[..actual.bytes as usize], &cpp.data[..cpp.bytes as usize], "cache bytes case{id}");
        assert_eq!(&actual.trace[..actual.events as usize], &cpp.trace[..cpp.events as usize], "complete empty/present style trace case{id}");
        std::println!("E {id} {} {}", fields(&actual).map(|n| n.to_string()).join(","), actual.trace[..actual.events as usize].iter().map(u32::to_string).collect::<Vec<_>>().join(","));
    }
}
fn witness(id: u32) {
    let out = empty_case(id, &input(id));
    assert!(out.bytes > 28);
    std::println!("Q {id} {} {} {} {}", out.status, out.pages, out.ends, out.data[28]);
}
#[test]
fn emit_absent_witness() { witness(0); }
#[test]
fn emit_present_witness() { witness(2); }
