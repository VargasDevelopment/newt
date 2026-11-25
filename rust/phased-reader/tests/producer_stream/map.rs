//! Same physical index: segments first, then original writeMap, no new backing.
use super::*;
struct MapStore<'a> {
    base: PassStore<'a>, id: u32, map_after: &'a Cell<u32>, map_bytes: &'a Cell<u32>,
}
impl CachePresence for MapStore<'_> {
    fn with_index<R>(&mut self, _: &[u8; 32], _: impl FnOnce(&mut dyn newt_phased_reader::index_build::Store) -> R) -> Option<R> { None }
    fn present(&mut self, n: &[u8], opening: bool) -> bool { self.base.present(n, opening) }
}
impl CacheStorage for MapStore<'_> {
    fn exists(&mut self, name: &[u8]) -> bool { self.present(name, false) }
    fn remove(&mut self, n: &[u8]) -> bool { self.base.remove(n) }
    fn file_size(&mut self, n: &[u8]) -> i64 { self.base.file_size(n) }
    fn read_at(&mut self, n: &[u8], at: u32, b: &mut [u8]) -> i32 { self.base.read_at(n, at, b) }
    fn begin_write(&mut self, n: &[u8]) -> bool {
        let map = self.base.card.borrow().begins == 3;
        if map && self.id == 2 {
            self.base.t.event(50); let mut card = self.base.card.borrow_mut();
            assert!(card.pending.is_none()); card.begins += 1; card.named(50, name(n));
            card.io.push(0); return false;
        }
        let ok = self.base.begin_write(n);
        if map && self.id == 5 { self.base.t.stop.set(true); }
        ok
    }
    fn write(&mut self, b: &[u8]) -> bool {
        if b.starts_with(b"NTSM") {
            self.map_after.set(self.base.card.borrow().files.len() as u32);
            self.map_bytes.set(b.len() as u32);
        }
        if self.id == 3 && self.base.card.borrow().begins == 4 {
            self.base.t.event(51); self.base.t.event(b.len() as u32);
            let mut card = self.base.card.borrow_mut(); card.io.push(51);
            u32_to(&mut card.io, b.len() as u32); card.io.extend_from_slice(b);
            card.io.push(0); return false;
        }
        self.base.write(b)
    }
    fn end_write(&mut self) -> bool {
        if self.id == 4 && self.base.card.borrow().begins == 4 {
            self.base.t.event(52); let mut card = self.base.card.borrow_mut();
            assert!(card.pending.is_some()); card.ends += 1;
            card.io.extend_from_slice(&[52, 0]); return false;
        }
        self.base.end_write()
    }
}
fn rust_map(id: u32, bytes: &[u8]) -> (ResultData, [u32; 2]) {
    let mut t = Trace::new(2000 + id); t.extended = Some(RefCell::new(Vec::new()));
    let card = RefCell::new(Card::default());
    let mut source = PassInput { base: Input { t: &t, bytes }, card: &card, id: if id == 6 { 3 } else { 0 } };
    let map_after = Cell::new(u32::MAX); let map_bytes = Cell::new(0);
    let mut base = MapStore { base: PassStore { t: &t, card: &card, id: 0 }, id, map_after: &map_after, map_bytes: &map_bytes };
    let mut selections = [Selection::default(), Selection::default()];
    selections[0].entry = newt_core::member_check::Checked::raw(bytes.len() as u32);
    let mut reader = PhasedReader::new(&mut selections, None, PageLayout::PORTRAIT);
    reader.source_bytes = bytes.len() as u64; reader.info.page_count = 257; reader.info.total_chars = 2570;
    let mut shared = [0; BANK_BYTES]; let mut native = vec![0xa5; newt_phased_sink::FRAME_BYTES];
    let signal = Signals::default(); let mut loan = Loan { bytes: &mut native, signal: &signal, returns: 0 };
    let mut storage = Storage::new(&mut shared); let mut parser = Native { t: &t, xml: Expat::default() };
    let mut breaker = linebreak::NativeBreak; let mut hash = PassHash(&t);
    let mut route = Route { settings: newt_core::preferences::ViewSettings { orientation: newt_core::preferences::Orientation::Portrait, ..Default::default() }, digest: &[0; 32], info: reader.info, hash: &mut hash };
    let cancel = Cancellation::new(Hooks { context: &t, stop: Some(stop), yield_now: Some(yield_now), pause: Some(pause) });
    let (mut seen, mut next, mut skipped, mut used, mut starts) = (0, 0, 0, 0, [0; 32]);
    let status = storage.with_producer(&mut loan, &cancel, &mut base, &mut source, 257,None, |owner| owner.with_buffered(|phase, mut a| {
        phase.defer(0)?; a.writer.segmented = true; a.writer.mapping = id != 1;
        a.state.language = Some(crate::BookLanguage::default());
        let status = a.produce_segments_and_map(&reader, phase, &mut parser, &mut breaker, &mut route);
        cancel.result(status == Status::Ok); a.store.remove(&a.state.name[..69]);
        seen = a.writer.seen; next = a.state.next; skipped = a.state.skipped;
        used = a.store.buffered_bytes() as u32; starts.copy_from_slice(a.writer.starts);
        assert_eq!(t.out.borrow().live, 0); assert!(card.borrow().pending.is_none()); Ok(status)
    })).unwrap();
    t.event(100); t.event(status as u32);
    let card = card.borrow(); let o = t.out.borrow();
    let state = [status as u32, 1, card.files.len() as u32, card.begins, card.ends, card.probes,
        o.creates, cancel.interrupted() as u32, card.pending.is_some() as u32, card.third_header as u32,
        seen, next, skipped, used, o.feeds, o.events];
    let mut data = Vec::new(); u32_to(&mut data, card.io.len() as u32); data.extend_from_slice(&card.io);
    u32_to(&mut data, card.files.len() as u32);
    for (n, b) in &card.files { u32_to(&mut data, n.len() as u32); data.extend_from_slice(n); u32_to(&mut data, b.len() as u32); data.extend_from_slice(b); }
    let trace = t.extended.as_ref().unwrap().borrow().clone();
    (ResultData { state, starts, data, trace }, [map_after.get(), map_bytes.get()])
}

#[test]
fn against_original_map() {
    let bytes = input();
    for id in 0..7 {
        let (cpp, facts) = captured_case(4, id, &bytes); let (got, observed) = rust_map(id, &bytes);
        assert_eq!(got.state, cpp.state, "state map{id}"); assert_eq!(got.starts, cpp.starts, "starts map{id}");
        assert_eq!(got.data, cpp.data, "every IO argument and complete file map{id}");
        assert_eq!(got.trace, cpp.trace, "complete trace map{id}"); assert_eq!(observed, facts, "map order/length map{id}");
        std::println!("M {id} {} {} {} {} {}", got.state.map(|x| x.to_string()).join(","),
            got.starts.map(|x| x.to_string()).join(","), got.data.iter().map(|x| std::format!("{x:02x}")).collect::<std::string::String>(),
            got.trace.iter().map(u32::to_string).collect::<Vec<_>>().join(","), observed.map(|x| x.to_string()).join(","));
    }
}
#[test]
fn emit_map_order_witness() {
    let (out, facts) = rust_map(0, &input());
    std::println!("M order {} {} {} {}", out.state[0], out.state[2], facts[0], facts[1]);
}
