extern crate std;
use super::super::{Selection,PhasedReader};
use crate::{phase_storage::{BANK_BYTES, BuildLoan, Storage}, imported_revision::RevisionHasher, CachePresence, PageLayout, producer::Route};
use newt_core::{producer_policy::{Cancellation, Hooks, Stop}, xml_session::{Handler, ParserBackend, ProducerBackend}, page_cache::CacheStorage, zip_container::{Entry,Status}, zip_window::source::BookSource};
use newt_target_expat::Expat;
use std::{cell::Cell, vec, vec::Vec};
#[path = "../../tests/producer_stream/linebreak.rs"]
mod linebreak;
#[derive(Default)]
struct Signals{pauses:Cell<u32>,feeds:Cell<u32>,live:Cell<bool>,cancel:Cell<bool>,producer:Cell<u32>}
struct Parser<'a>{xml:Expat,signal:&'a Signals,fail:bool}
impl ParserBackend for Parser<'_>{
    fn producer_backend(&mut self) -> Option<&mut dyn ProducerBackend> { Some(self) }
 fn create(&mut self)->bool{panic!("producer must select the bounded parser")}
 fn feed(&mut self,b:&[u8],last:bool,h:&mut dyn Handler)->bool{assert!(self.signal.live.get());self.signal.feeds.set(self.signal.feeds.get()+1);self.xml.feed(b,last,h)}
 fn close(&mut self){self.xml.close();self.signal.live.set(false)}
}
impl ProducerBackend for Parser<'_>{
 fn create_producer(&mut self)->bool{self.signal.producer.set(self.signal.producer.get()+1);if self.fail{return false}let ok=self.xml.create_producer();self.signal.live.set(ok);ok}
 fn current_byte_index(&self)->i64{self.xml.current_byte_index()}
}
struct Loan<'a>{bytes:&'a mut[u8],signal:&'a Signals,returns:u32}
impl BuildLoan for Loan<'_>{fn with_parse<R>(&mut self,f:impl for<'a>FnOnce(&'a mut[u8])->R)->Result<R,Status>{let r=f(self.bytes);assert!(!self.signal.live.get());self.bytes.fill(0xff);self.returns+=1;Ok(r)}}
struct Source{bytes:Vec<u8>,fail:bool}
impl BookSource for Source{
 fn size(&self)->u64{self.bytes.len() as u64}
 fn read_at(&mut self,at:u64,out:&mut[u8])->i32{if self.fail{return -1}let at=usize::try_from(at).unwrap().min(self.bytes.len());let n=out.len().min(self.bytes.len()-at);out[..n].copy_from_slice(&self.bytes[at..at+n]);n as i32}
}
#[derive(Default)]
struct Cache{bytes:Vec<u8>,begins:u32,ends:u32}
impl CacheStorage for Cache{
    fn exists(&mut self, name: &[u8]) -> bool { self.present(name, false) }
 fn file_size(&mut self,_:&[u8])->i64{self.bytes.len() as i64}
 fn read_at(&mut self,_:&[u8],_:u32,_:&mut[u8])->i32{-1}
 fn begin_write(&mut self,_:&[u8])->bool{self.begins+=1;true}
 fn write(&mut self,b:&[u8])->bool{self.bytes.extend_from_slice(b);true}
 fn end_write(&mut self)->bool{self.ends+=1;true}
 fn remove(&mut self,_:&[u8])->bool{true}
}
impl CachePresence for Cache{fn present(&mut self,_:&[u8],_:bool)->bool{false}}
struct Hash;
impl RevisionHasher for Hash{
 fn hash(&mut self,_:&[u8],_:&mut[u8;32])->bool{panic!("one-file stream does not name a segment")}
 fn begin(&mut self)->bool{false}fn update(&mut self,_:&[u8])->bool{false}fn finish(&mut self,_:&mut[u8;32])->bool{false}
}
#[derive(Default)]
struct Context{signal:Signals,cancel_pause:bool}
fn stopped(c:&Context)->Stop{if c.signal.cancel.get(){Stop::Interrupted}else{Stop::None}}
fn paused(c:&Context,_:&Cancellation<'_,Context>)->Stop{c.signal.pauses.set(c.signal.pauses.get()+1);if c.cancel_pause{Stop::Interrupted}else{Stop::None}}
fn one(label:&str){
 let input=match label{
  "malformed"=>b"<html><body><p>A</body></html>".to_vec(),
  "doctype"=>b"<!DOCTYPE html [<!ENTITY e 'A'>]><html><body><p>&e;</p></body></html>".to_vec(),
  "budget"=>{let mut s=b"<html><body>".to_vec();for _ in 0..1024{s.extend_from_slice(b"<!--z-->");}s.extend_from_slice(b"<p>A</p></body></html>");s},
  _=>b"<html><body><p>A</p></body></html>".to_vec()
 };
 let mut selections=[Selection::default(),Selection::default()];selections[0].entry=Entry::raw(input.len() as u32);
 let mut reader=PhasedReader::new(&mut selections,None,PageLayout::PORTRAIT);reader.info.page_count=if label=="pages"{2}else{1};reader.info.total_chars=if label=="chars"{2}else{1};
 let info=reader.info;let mut hash=Hash;let mut route=Route{settings:newt_core::preferences::ViewSettings{orientation:newt_core::preferences::Orientation::Portrait,..Default::default()},digest:&[0;32],info,hash:&mut hash};
 let context=Context{cancel_pause:label=="cancel",..Default::default()};let cancellation=Cancellation::new(Hooks{context:&context,stop:Some(stopped),yield_now:None,pause:Some(paused)});
 let mut native=vec![0xa5;newt_phased_sink::FRAME_BYTES];let mut loan=Loan{bytes:&mut native,signal:&context.signal,returns:0};let mut shared=[0;BANK_BYTES];let mut storage=Storage::new(&mut shared);
 let mut source=Source{bytes:input,fail:label=="read"};let mut cache=Cache::default();let mut parser=Parser{xml:Expat::default(),signal:&context.signal,fail:label=="create"};let mut breaker=linebreak::NativeBreak;
 let mut pages=0;let mut closed=false;let status=storage.with_producer(&mut loan,&cancellation,&mut cache,&mut source,info.page_count,None,|owner| owner.with_buffered(|phase,mut attempt|{attempt.with_writer(|a|{
  phase.defer(0)?;assert!(a.writer.cache.begin(a.store,b"c",123,&a.memory));
  let status=a.stream_current(&reader,phase,&mut parser,&mut breaker,&mut route);pages=a.writer.cache.page_count();
  assert!(!context.signal.live.get(),"parser closed before finish and before loan return");closed=true;
  if status==Status::Ok{a.writer.cache.set_total_chars(info.total_chars);assert!(a.writer.cache.finish(a.store,b"c",&a.memory));}
  Ok(status)
 })?})).unwrap();
 assert_eq!(reader.info,info);assert!(!reader.has_page());assert_eq!(cache.begins,1);assert!(loan.bytes.iter().all(|&b|b==0xff));
 std::println!("O {label} {} {pages} {} {} {} {} {} {} {}",status as u32,context.signal.pauses.get(),context.signal.feeds.get(),context.signal.producer.get(),loan.returns,closed as u8,cancellation.interrupted() as u8,cache.ends);
}
#[test]
fn emit_rows(){let selected=std::env::var("NEWT_STREAM_CASE").ok();for label in ["whole","pages","chars","malformed","doctype","cancel","read","create","budget"]{if selected.as_deref().is_none_or(|s|s==label){one(label)}}}

#[cfg(test)]
#[path = "producer_policy_trace_cases.rs"]
mod traces;

// This SDK-fake source has no native receipt; the capability is explicit.
impl newt_core::producer_policy::ProducerReceipt for Source {
    fn producer_receipt_live(&mut self) -> bool { true }
}
