//! Host-only captured C++ stream/source comparison; not an application owner.
use super::*;
use newt_core::admission::{ScheduledSource,ChapterSource};
use std::cell::RefCell;
use std::string::ToString;
#[repr(C)]
struct Output{status:u32,pages:u32,pauses:u32,feeds:u32,creates:u32,live:u32,interrupted:u32,ends:u32,polls:u32,yields:u32,bytes:u32,events:u32,data:[u8;4096],trace:[u32;8192]}
impl Default for Output{fn default()->Self{Self{status:0,pages:0,pauses:0,feeds:0,creates:0,live:0,interrupted:0,ends:0,polls:0,yields:0,bytes:0,events:0,data:[0;4096],trace:[0;8192]}}}
#[path = "producer_expected.rs"]
mod expected;
fn captured_output(kind: u32, id: u32, input: &[u8]) -> (Output, u32) {
 let record = expected::load(kind, id, input);
 let [status,pages,pauses,feeds,creates,live,interrupted,ends,polls,yields,bytes,events] = record.fields;
 let mut out = Output { status,pages,pauses,feeds,creates,live,interrupted,ends,polls,yields,bytes,events,..Output::default() };
 out.data[..record.data.len()].copy_from_slice(record.data);
 out.trace[..record.trace.len()].copy_from_slice(&record.trace);
 (out, record.probes)
}
struct Trace{out:RefCell<Output>,id:u32,armed:Cell<u32>,stop:Cell<bool>,extended:Option<RefCell<Vec<u32>>>}
impl Trace{
 fn new(id:u32)->Self{Self{out:RefCell::new(Output::default()),id,armed:Cell::new(0),stop:Cell::new(false),extended:None}}
 fn event(&self,n:u32){if let Some(v)=&self.extended{assert!(v.borrow().len()<262144);v.borrow_mut().push(n);self.out.borrow_mut().events+=1;return;}let mut o=self.out.borrow_mut();let at=o.events as usize;assert!(at<o.trace.len());o.trace[at]=n;o.events+=1;}
}
fn stop(t:&Trace)->Stop{t.out.borrow_mut().polls+=1;if t.armed.get()!=0{let n=t.armed.get()-1;t.armed.set(n);if n==0{t.stop.set(true);}t.event(10);return Stop::None}t.event(if t.stop.get(){11}else{10});if t.stop.get(){Stop::Interrupted}else{Stop::None}}
fn yield_now(t:&Trace){t.out.borrow_mut().yields+=1;t.event(20)}
fn pause(t:&Trace,_:&Cancellation<'_,Trace>)->Stop{t.out.borrow_mut().pauses+=1;t.event(30);if t.id==9 && t.out.borrow().pauses==2{t.armed.set(2)}if t.id==5{Stop::Interrupted}else{Stop::None}}
struct Input<'a>{t:&'a Trace,bytes:&'a[u8]}
impl BookSource for Input<'_>{
 fn size(&self)->u64{self.bytes.len() as u64}
 fn read_at(&mut self,at:u64,out:&mut[u8])->i32{self.t.event(40);self.t.event(at as u32);self.t.event(out.len() as u32);if self.t.id==6 || self.t.id==11{self.t.event(u32::MAX);return -1}let at=usize::try_from(at).unwrap().min(self.bytes.len());let n=out.len().min(self.bytes.len()-at);out[..n].copy_from_slice(&self.bytes[at..at+n]);self.t.event(n as u32);n as i32}
}
struct StorageOut<'a>(&'a Trace);
impl CacheStorage for StorageOut<'_>{
    fn exists(&mut self, name: &[u8]) -> bool { self.present(name, false) }
 fn remove(&mut self,_:&[u8])->bool{self.0.event(53);true}
 fn file_size(&mut self,_:&[u8])->i64{self.0.out.borrow().bytes as i64}
 fn read_at(&mut self,_:&[u8],_:u32,_:&mut[u8])->i32{-1}
 fn begin_write(&mut self,_:&[u8])->bool{self.0.event(50);true}
 fn write(&mut self,b:&[u8])->bool{self.0.event(51);self.0.event(b.len() as u32);if self.0.id==21{return false;}let mut o=self.0.out.borrow_mut();let at=o.bytes as usize;assert!(at+b.len()<=o.data.len());o.data[at..at+b.len()].copy_from_slice(b);o.bytes+=b.len() as u32;true}
 fn end_write(&mut self)->bool{self.0.event(52);self.0.out.borrow_mut().ends+=1;true}
}
impl CachePresence for StorageOut<'_>{fn present(&mut self,_:&[u8],_:bool)->bool{false}}
struct Native<'a>{t:&'a Trace,xml:Expat}
impl ParserBackend for Native<'_>{
    fn producer_backend(&mut self) -> Option<&mut dyn ProducerBackend> { Some(self) }
 fn create(&mut self)->bool{panic!("producer parser required")}
 fn feed(&mut self,b:&[u8],last:bool,h:&mut dyn Handler)->bool{self.t.out.borrow_mut().feeds+=1;self.t.event(61);self.t.event(b.len() as u32);self.t.event(last as u32);self.xml.feed(b,last,h)}
 fn close(&mut self){if self.t.out.borrow().live!=0{self.t.event(62);self.t.out.borrow_mut().live=0;}self.xml.close();}
}
impl ProducerBackend for Native<'_>{
 fn create_producer(&mut self)->bool{self.t.out.borrow_mut().creates+=1;self.t.event(60);if self.t.id==7{return false}let ok=self.xml.create_producer();self.t.out.borrow_mut().live=ok as u32;ok}
 fn current_byte_index(&self)->i64{self.xml.current_byte_index()}
}
fn bytes(id:u32)->Vec<u8>{match id{3=>b"<html><body><p>A</body></html>".to_vec(),4=>b"<!DOCTYPE html [<!ENTITY e 'A'>]><html><body><p>&e;</p></body></html>".to_vec(),8=>{let mut b=b"<html><body>".to_vec();for _ in 0..1024{b.extend_from_slice(b"<!--z-->")}b.extend_from_slice(b"<p>A</p></body></html>");b},_=>b"<html><body><p>A</p></body></html>".to_vec()}}
fn rust(id:u32,b:&[u8])->Output{
 let t=Trace::new(id);let cancel=Cancellation::new(Hooks{context:&t,stop:Some(stop),yield_now:Some(yield_now),pause:Some(pause)});let mut input=Input{t:&t,bytes:b};
 if id>=10 && id<21{
  let ordinary=id==18 || id==19;let mut producer=newt_core::producer_policy::ProducerBookSource::new(&mut input,&cancel);
  let input:&mut dyn BookSource=if ordinary{&mut Input{t:&t,bytes:b}}else{&mut producer};
  let policy:Option<&dyn newt_core::xml_session::SourcePolicy>=if ordinary{None}else{Some(&cancel)};
  let mut scheduled=ScheduledSource::new(input,None,policy);let mut output=[0;3000];
  let status=if matches!(id,10|11|12|18){let got=scheduled.read_at(if id==12{b.len() as u64}else{0},&mut output[..if id==12{0}else{3000}]);if id==11{let _=scheduled.read_at(0,&mut output[..1]);}if got<0{Status::IoError}else{Status::Ok}}
   else if id==20{if producer.read_at(0,&mut output)<0{Status::IoError}else{Status::Ok}}
   else{let entry=Entry::raw(b.len() as u32);let mut chapter=ChapterSource::open(&mut scheduled,&entry,None,None,policy).unwrap();if id==17{t.stop.set(true)}let got=chapter.read_at(if id==14{1}else{0},&mut output[..if id==15{1}else{3000}]);if got<0{Status::IoError}else{if id==16{t.stop.set(true)}if chapter.finish(){Status::Ok}else{Status::Truncated}}};
  t.out.borrow_mut().status=status as u32;t.out.borrow_mut().interrupted=cancel.interrupted() as u32;return t.out.into_inner();
 }
 let mut shared=[0;BANK_BYTES];let mut native=vec![0xa5;newt_phased_sink::FRAME_BYTES];let signal=Signals::default();let mut loan=Loan{bytes:&mut native,signal:&signal,returns:0};let mut storage=Storage::new(&mut shared);
 let mut selections=[Selection::default(),Selection::default()];selections[0].entry=Entry::raw(b.len() as u32);let mut reader=PhasedReader::new(&mut selections,None,PageLayout::PORTRAIT);reader.info.page_count=if id==1{2}else{1};reader.info.total_chars=if id==2{2}else{1};
 let mut hash=Hash;let mut route=Route{settings:newt_core::preferences::ViewSettings{orientation:newt_core::preferences::Orientation::Portrait,..Default::default()},digest:&[0;32],info:reader.info,hash:&mut hash};let mut out=StorageOut(&t);let mut parser=Native{t:&t,xml:Expat::default()};let mut breaker=linebreak::NativeBreak;
 let status=storage.with_producer(&mut loan,&cancel,&mut out,&mut input,reader.info.page_count,None,|owner| owner.with_buffered(|phase,mut attempt|{attempt.with_writer(|a|{phase.defer(0)?;if !a.writer.cache.begin(a.store,b"c",123,&a.memory){assert_eq!(id,21);return Ok(Status::OutOfMemory);}let mut status=a.stream_current(&reader,phase,&mut parser,&mut breaker,&mut route);t.out.borrow_mut().pages=a.writer.cache.page_count();assert_eq!(t.out.borrow().live,0);if status==Status::Ok{a.writer.cache.set_total_chars(reader.info.total_chars);let finished=a.writer.cache.finish(a.store,b"c",&a.memory);if id==21 && !finished{status=Status::IoError;}else{assert!(finished);}}Ok(status)})?})).unwrap();
 t.out.borrow_mut().status=status as u32;t.out.borrow_mut().interrupted=cancel.interrupted() as u32;t.out.into_inner()
}
#[test]
fn against_original(){
 for id in 0..22{let b=bytes(id);let (cpp, _)=captured_output(0,id,&b);let actual=rust(id,&b);
  let fields=|o:&Output|[o.status,o.pages,o.pauses,o.feeds,o.creates,o.live,o.interrupted,o.ends,o.polls,o.yields,o.bytes,o.events];
  assert_eq!(fields(&actual),fields(&cpp),"fields case{id}");assert_eq!(&actual.data[..actual.bytes as usize],&cpp.data[..cpp.bytes as usize],"cache bytes case{id}");assert_eq!(&actual.trace[..actual.events as usize],&cpp.trace[..cpp.events as usize],"complete poll/read/yield trace case{id}");
  std::println!("D {id} {} {}",fields(&actual).map(|n|n.to_string()).join(","),actual.trace[..actual.events as usize].iter().map(u32::to_string).collect::<Vec<_>>().join(","));
 }
}

#[test]
fn emit_header_witness(){let out=rust(21,&bytes(21));std::println!("H {} {} {} {}",out.status,out.creates,out.pages,out.bytes);}

#[cfg(test)]
#[path = "producer_publication_cases.rs"]
mod publication;

#[path = "../tests/producer_stream/empty_styles.rs"]
mod empty_styles;

#[path = "../tests/producer_stream/passes.rs"]
mod passes;
#[path = "../tests/producer_stream/book_language.rs"]
mod book_language;

// This SDK-fake source has no native receipt; the capability is explicit.
impl newt_core::producer_policy::ProducerReceipt for Input<'_> {
    fn producer_receipt_live(&mut self) -> bool { true }
}
