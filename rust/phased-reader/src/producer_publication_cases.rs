//! Host-only finishWhole comparison; the original source stays private in Core.
use super::*;

std::thread_local! { static AFTER_STREAM: Cell<bool> = const { Cell::new(false) }; }
fn work(event: newt_core::admission::WorkEvent) {
    use newt_core::admission::{WorkKind, WorkPhase};
    if event.kind == WorkKind::Phase && event.phase == WorkPhase::Idle { AFTER_STREAM.set(true); }
}
fn publication_stop(t: &Trace) -> Stop {
    if t.id == 25 && AFTER_STREAM.get() { t.stop.set(true); }
    stop(t)
}
fn closed(t: &Trace) -> bool {
    let out = t.out.borrow();
    out.creates != 0 && out.live == 0 && AFTER_STREAM.get()
}
fn publication_yield(t: &Trace) {
    yield_now(t);
    if t.id == 32 && closed(t) { t.stop.set(true); }
}
struct PublicationInput<'a> { inner: Input<'a>, probes: &'a Cell<u32> }
impl BookSource for PublicationInput<'_> {
    fn size(&self) -> u64 {
        self.inner.size() + u64::from(self.inner.t.id == 23 && closed(self.inner.t))
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let t = self.inner.t;
        if closed(t) && out.is_empty() {
            self.probes.set(self.probes.get() + 1);
            if t.id == 24 || t.id == 31 {
                t.event(40); t.event(at as u32); t.event(0);
                let got = if t.id == 24 { -1 } else { 1 };
                t.event(got as u32);
                return got;
            }
            let got = self.inner.read_at(at, out);
            if t.id == 26 { t.stop.set(true); }
            got
        } else { self.inner.read_at(at, out) }
    }
}
struct PublicationStore<'a>(StorageOut<'a>);
impl CacheStorage for PublicationStore<'_> {
    fn exists(&mut self, name: &[u8]) -> bool { self.present(name, false) }
    fn remove(&mut self, n: &[u8]) -> bool { self.0.remove(n) }
    fn file_size(&mut self, n: &[u8]) -> i64 { self.0.file_size(n) }
    fn read_at(&mut self, n: &[u8], at: u32, b: &mut [u8]) -> i32 { self.0.read_at(n, at, b) }
    fn begin_write(&mut self, n: &[u8]) -> bool { self.0.begin_write(n) }
    fn write(&mut self, b: &[u8]) -> bool {
        let t = self.0.0;
        if t.id == 27 && closed(t) { t.event(51); t.event(b.len() as u32); return false; }
        let ok = self.0.write(b);
        if t.id == 33 && closed(t) { t.stop.set(true); }
        ok
    }
    fn end_write(&mut self) -> bool { let ok = self.0.end_write(); ok && self.0.0.id != 28 }
}
impl CachePresence for PublicationStore<'_> {
    fn present(&mut self, n: &[u8], opening: bool) -> bool { self.0.present(n, opening) }
}
fn publication(id: u32, b: &[u8]) -> (Output, u32) {
    AFTER_STREAM.set(false);
    let t = Trace::new(id);
    let probes = Cell::new(0);
    let cancel = Cancellation::new(Hooks { context: &t, stop: Some(publication_stop), yield_now: Some(publication_yield), pause: Some(pause) });
    let mut input = PublicationInput { inner: Input { t: &t, bytes: b }, probes: &probes };
    let mut shared = [0; BANK_BYTES];
    let mut native = vec![0xa5; newt_phased_sink::FRAME_BYTES];
    let signal = Signals::default();
    let mut loan = Loan { bytes: &mut native, signal: &signal, returns: 0 };
    let mut storage = Storage::new(&mut shared);
    let mut selections = [Selection::default(), Selection::default()];
    selections[0].entry = Entry::raw(b.len() as u32);
    let mut reader = PhasedReader::new(&mut selections, None, PageLayout::PORTRAIT);
    reader.source_bytes = b.len() as u64;
    reader.work = Some(work);
    reader.info.page_count = if id == 29 { 2 } else { 1 };
    reader.info.total_chars = 1;
    let mut hash = Hash;
    let mut route = Route { settings: newt_core::preferences::ViewSettings { orientation: newt_core::preferences::Orientation::Portrait, ..Default::default() }, digest: &[0; 32], info: reader.info, hash: &mut hash };
    let mut out = PublicationStore(StorageOut(&t));
    let mut parser = Native { t: &t, xml: Expat::default() };
    let mut breaker = linebreak::NativeBreak;
    let status = storage.with_producer(&mut loan, &cancel, &mut out, &mut input, reader.info.page_count,None, |owner| owner.with_buffered(|phase, mut attempt| { attempt.with_writer(|a| {
        phase.defer(0)?;
        a.state.name[0] = b'c';
        assert!(a.writer.cache.begin(a.store, &a.state.name[..69], 123, &a.memory));
        a.writer.segmented = id == 30;
        let status = a.finish_current(&reader, phase, &mut parser, &mut breaker, &mut route);
        t.out.borrow_mut().pages = a.writer.cache.page_count();
        assert_eq!(t.out.borrow().live, 0);
        Ok(status)
    })? })).unwrap();
    t.out.borrow_mut().status = status as u32;
    t.out.borrow_mut().interrupted = cancel.interrupted() as u32;
    (t.out.into_inner(), probes.get())
}
fn fields(o: &Output) -> [u32; 12] {
    [o.status,o.pages,o.pauses,o.feeds,o.creates,o.live,o.interrupted,o.ends,o.polls,o.yields,o.bytes,o.events]
}
#[test]
fn against_original_finish() {
    for id in 22..34 {
        let b = bytes(0);
        let (cpp, probes) = captured_output(1, id, &b);
        let (actual, got) = publication(id, &b);
        assert_eq!(fields(&actual), fields(&cpp), "fields case{id}");
        assert_eq!(got, probes, "source probes case{id}");
        assert_eq!(&actual.data[..actual.bytes as usize], &cpp.data[..cpp.bytes as usize], "cache bytes case{id}");
        assert_eq!(&actual.trace[..actual.events as usize], &cpp.trace[..cpp.events as usize], "complete final publication trace case{id}");
        std::println!("F {id} {} {got} {}", fields(&actual).map(|x|x.to_string()).join(","), actual.trace[..actual.events as usize].iter().map(u32::to_string).collect::<Vec<_>>().join(","));
    }
}
#[test]
fn emit_gate_witness() {
    let (out, probes) = publication(24, &bytes(0));
    std::println!("P {} {} {} {} {}", out.status, out.pages, out.ends, probes, out.interrupted);
}

// This SDK-fake source has no native receipt; the capability is explicit.
impl newt_core::producer_policy::ProducerReceipt for PublicationInput<'_> {
    fn producer_receipt_live(&mut self) -> bool { true }
}
