//! Exact original produceSegments with real XML/layout/serialization; fake card.
use super::*;
use std::collections::BTreeMap;
unsafe extern "C" {
    fn producer_passes_hash(b: *const u8, n: u32, output: *mut u8) -> bool;
}
#[derive(Default)]
struct Card {
    files: BTreeMap<Vec<u8>, Vec<u8>>, pending: Option<(Vec<u8>, Vec<u8>)>, io: Vec<u8>,
    begins: u32, ends: u32, probes: u32, third_header: bool,
}
fn name(n: &[u8]) -> &[u8] { n.split(|b| *b == 0).next().unwrap() }
fn u32_to(out: &mut Vec<u8>, n: u32) { out.extend_from_slice(&n.to_le_bytes()); }
impl Card {
    fn named(&mut self, code: u8, n: &[u8]) {
        self.io.push(code); u32_to(&mut self.io, n.len() as u32); self.io.extend_from_slice(n);
    }
}
struct PassInput<'a> { base: Input<'a>, card: &'a RefCell<Card>, id: u32 }
impl BookSource for PassInput<'_> {
    fn size(&self) -> u64 { self.base.size() }
    fn read_at(&mut self, at: u64, b: &mut [u8]) -> i32 {
        if b.is_empty() {
            let mut card = self.card.borrow_mut(); card.probes += 1;
            if self.id == 3 && card.probes == 1 {
                self.base.t.event(40); self.base.t.event(at as u32); self.base.t.event(0); self.base.t.event(u32::MAX); return -1;
            }
        }
        let got = self.base.read_at(at, b);
        if matches!(self.id, 4 | 8) && b.is_empty() && self.card.borrow().probes == 3 { self.base.t.stop.set(true); }
        got
    }
}
struct PassStore<'a> { t: &'a Trace, card: &'a RefCell<Card>, id: u32 }
impl CachePresence for PassStore<'_> {
    fn present(&mut self, n: &[u8], opening: bool) -> bool {
        assert!(!opening); self.t.event(54); let n = name(n); let mut card = self.card.borrow_mut();
        let found = card.files.contains_key(n); card.named(54, n); card.io.push(found as u8); found
    }
}
impl CacheStorage for PassStore<'_> {
    fn exists(&mut self, name: &[u8]) -> bool { self.present(name, false) }
    fn remove(&mut self, n: &[u8]) -> bool { self.t.event(53); let mut card=self.card.borrow_mut();card.named(53,name(n));card.pending=None;true }
    fn file_size(&mut self, _: &[u8]) -> i64 { panic!("producer presence has no size call") }
    fn read_at(&mut self, _: &[u8], _: u32, _: &mut [u8]) -> i32 { panic!("producer presence has no read call") }
    fn begin_write(&mut self, n: &[u8]) -> bool {
        self.t.event(50);let mut card=self.card.borrow_mut();assert!(card.pending.is_none());card.begins+=1;card.named(50,name(n));
        let ok=self.id!=9;card.io.push(ok as u8);if ok{card.pending=Some((name(n).to_vec(),Vec::new()));}ok
    }
    fn write(&mut self, b: &[u8]) -> bool {
        self.t.event(51);self.t.event(b.len() as u32);let mut card=self.card.borrow_mut();
        if card.begins==3 && card.pending.as_ref().unwrap().1.is_empty(){card.third_header=b.len()==12 && self.t.out.borrow().creates==1;}
        let ok=!(self.id==5 && card.begins==3);card.io.push(51);u32_to(&mut card.io,b.len() as u32);card.io.extend_from_slice(b);card.io.push(ok as u8);
        if ok{card.pending.as_mut().unwrap().1.extend_from_slice(b);}ok
    }
    fn end_write(&mut self) -> bool {
        self.t.event(52);let mut card=self.card.borrow_mut();assert!(card.pending.is_some());card.ends+=1;let ok=!(self.id==6 && card.ends==3);card.io.extend_from_slice(&[52,ok as u8]);
        if ok{let(n,b)=card.pending.take().unwrap();card.files.insert(n,b);}ok
    }
}
pub(super) struct PassHash<'a>(pub(super) &'a Trace);
impl RevisionHasher for PassHash<'_> {
    fn begin(&mut self) -> bool { false }
    fn update(&mut self, _: &[u8]) -> bool { false }
    fn finish(&mut self, _: &mut [u8; 32]) -> bool { false }
    fn hash(&mut self, b: &[u8], d: &mut [u8; 32]) -> bool {
        self.0.event(90);self.0.event(b.len() as u32);
        // SAFETY: synchronous host SHA reads a live slice and writes32 disjoint
        // output bytes; neither pointer is retained.
        unsafe { producer_passes_hash(b.as_ptr(),b.len() as u32,d.as_mut_ptr()) }
    }
}
struct ResultData { state: [u32;16], starts: [u32;32], data: Vec<u8>, trace: Vec<u32> }
fn input() -> Vec<u8> {
    let mut b=b"<html><body>".to_vec();for _ in 0..2570{b.extend_from_slice(b"A<hr/>");}b.extend_from_slice(b"</body></html>");b
}
fn rust_case(id:u32,b:&[u8])->ResultData {
    let mut t=Trace::new(1000+id);t.extended=Some(RefCell::new(Vec::new()));let card=RefCell::new(Card::default());
    let mut source=PassInput{base:Input{t:&t,bytes:b},card:&card,id};let mut base=PassStore{t:&t,card:&card,id};
    let mut selections=[Selection::default(),Selection::default()];selections[0].entry=Entry::raw(b.len() as u32);let mut reader=PhasedReader::new(&mut selections,None,false);reader.source_bytes=b.len() as u64;reader.info.page_count=257;reader.info.total_chars=2570;
    let mut shared=[0;BANK_BYTES];let mut native=vec![0xa5;newt_phased_sink::FRAME_BYTES];let signal=Signals::default();let mut loan=Loan{bytes:&mut native,signal:&signal,returns:0};let mut storage=Storage::new(&mut shared);
    let mut parser=Native{t:&t,xml:Expat::default()};let mut breaker=linebreak::NativeBreak;let mut hash=PassHash(&t);let mut route=Route{settings:newt_core::preferences::ViewSettings{orientation:newt_core::preferences::Orientation::Portrait,..Default::default()},digest:&[0;32],info:reader.info,hash:&mut hash};
    let(mut seen,mut next,mut skipped,mut used,mut starts)=(0,0,0,0,[0;32]);let mut interruptions=0;let mut calls=0;
    let mut attempt=||{
        calls+=1;let cancel=Cancellation::new(Hooks{context:&t,stop:Some(stop),yield_now:Some(yield_now),pause:Some(pause)});
        let status=storage.with_producer(&mut loan,&cancel,&mut base,&mut source,257,None,|owner| owner.with_buffered(|phase,mut attempt|{attempt.with_writer(|a|{
            phase.defer(0)?;a.writer.segmented=true;a.writer.mapping=id==1;
            let status=a.produce_segments(&reader,phase,&mut parser,&mut breaker,&mut route);cancel.result(status==Status::Ok);a.store.remove(&a.state.name[..69]);
            seen=a.writer.seen;next=a.state.next;skipped=a.state.skipped;used=a.store.buffered_bytes() as u32;starts.copy_from_slice(a.writer.starts);
            assert_eq!(t.out.borrow().live,0);assert!(card.borrow().pending.is_none());Ok(status)
        })?})).unwrap();interruptions+=u32::from(cancel.interrupted());t.event(100);t.event(status as u32);status
    };
    if id==2{t.stop.set(true);}let mut status=attempt();if id==7 || id==8{t.stop.set(false);status=attempt();}drop(attempt);
    let card=card.borrow();let o=t.out.borrow();let state=[status as u32,calls,card.files.len() as u32,card.begins,card.ends,card.probes,o.creates,interruptions,card.pending.is_some() as u32,card.third_header as u32,seen,next,skipped,used,o.feeds,o.events];
    let mut data=Vec::new();u32_to(&mut data,card.io.len() as u32);data.extend_from_slice(&card.io);u32_to(&mut data,card.files.len() as u32);
    for(n,b)in &card.files{u32_to(&mut data,n.len() as u32);data.extend_from_slice(n);u32_to(&mut data,b.len() as u32);data.extend_from_slice(b);}
    let trace=t.extended.as_ref().unwrap().borrow().clone();ResultData{state,starts,data,trace}
}
fn captured_case(kind: u32, id: u32, bytes: &[u8]) -> (ResultData, [u32; 2]) {
    let record = expected::load(kind, id, bytes);
    (ResultData { state: record.state, starts: record.starts,
        data: record.data.to_vec(), trace: record.trace }, record.facts)
}

#[test]
fn against_original(){
    let b=input();for id in 0..10{let (cpp,_)=captured_case(3,id,&b);let got=rust_case(id,&b);assert_eq!(got.state,cpp.state,"state case{id}");assert_eq!(got.starts,cpp.starts,"starts case{id}");assert_eq!(got.data,cpp.data,"every IO argument and complete file case{id}");assert_eq!(got.trace,cpp.trace,"complete trace case{id}");
        std::println!("S {id} {} {} {} {}",got.state.map(|x|x.to_string()).join(","),got.starts.map(|x|x.to_string()).join(","),got.data.iter().map(|x|std::format!("{x:02x}")).collect::<std::string::String>(),got.trace.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
    }
}
#[test]
fn emit_pass_witness(){let out=rust_case(0,&input());std::println!("C pass {} {} {} {}",out.state[0],out.state[2],out.state[4],out.state[6]);}
#[test]
fn emit_header_witness(){let out=rust_case(5,&input());std::println!("C header {}",out.state[9]);}
#[test]
fn emit_source_witness(){let out=rust_case(3,&input());std::println!("C source {} {} {}",out.state[0],out.state[2],out.state[6]);}

#[path = "map.rs"]
mod map;

// This SDK-fake source has no native receipt; the capability is explicit.
impl newt_core::producer_policy::ProducerReceipt for PassInput<'_> {
    fn producer_receipt_live(&mut self) -> bool { true }
}
