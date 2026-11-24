// SPDX-License-Identifier: MIT
//! Frozen DEV-only cover counters, monitor lease, rows and read-only census.
#![forbid(unsafe_code)]
use core::{cell::Cell,fmt::{self,Write}};
use newt_native_io::{UiIo,cover::observer as sdk};
use newt_native_storage::Kind;
use super::{io::Control,CoverResult as R};
use super::heap::Heap;
/// The single synchronous idle call's exact frozen observation fields.
pub struct Observation{
    started:u32,elapsed:Cell<u32>,reads:Cell<usize>,read_bytes:Cell<usize>,
    writes:Cell<usize>,write_bytes:Cell<usize>,opens:Cell<usize>,path_stats:Cell<usize>,
    min_free:Cell<usize>,min_largest:Cell<usize>,monitor_start:Cell<i32>,monitor_stop:Cell<i32>,
}
impl Observation{
    pub fn new(started:u32)->Self{Self{started,elapsed:Cell::new(0),reads:Cell::new(0),read_bytes:Cell::new(0),
        writes:Cell::new(0),write_bytes:Cell::new(0),opens:Cell::new(0),path_stats:Cell::new(0),
        min_free:Cell::new(usize::MAX),min_largest:Cell::new(usize::MAX),monitor_start:Cell::new(-1),monitor_stop:Cell::new(-1)}}
    pub fn open(&self){self.opens.set(self.opens.get().wrapping_add(1));}
    pub fn stat(&self){self.path_stats.set(self.path_stats.get().wrapping_add(1));}
    pub fn read(&self){
        self.reads.set(self.reads.get().wrapping_add(1));
    }
    pub fn read_result(&self,returned:Option<usize>){
        if let Some(bytes)=returned{self.read_bytes.set(self.read_bytes.get().wrapping_add(bytes));}
    }
    pub fn write(&self){
        self.writes.set(self.writes.get().wrapping_add(1));
    }
    pub fn write_result(&self,returned:Option<usize>){
        if let Some(bytes)=returned{self.write_bytes.set(self.write_bytes.get().wrapping_add(bytes));}
    }
    pub fn sample_largest(&self,actual:usize){self.min_largest.set(self.min_largest.get().min(actual));}
    pub fn finish_time(&self,now:u32){self.elapsed.set(now.wrapping_sub(self.started));}
}
#[cfg(target_pointer_width="32")]
const _:()=assert!(core::mem::size_of::<Observation>()==48);
/// The actual SDK local-heap monitor; no global diagnostic controller.
pub struct Monitor<'a>{observation:&'a Observation,active:bool}
impl<'a> Monitor<'a>{
    pub fn start(observation:&'a Observation)->Self{
        observation.sample_largest(sdk::monitor_largest());
        let status=sdk::monitor_start();observation.monitor_start.set(status);
        Self{observation,active:status==0}
    }
    pub fn finish(&mut self){
        if !self.active{return;}
        self.observation.sample_largest(sdk::monitor_largest());
        self.observation.min_free.set(sdk::monitor_minimum());
        let status=sdk::monitor_stop();self.observation.monitor_stop.set(status);self.active=false;
    }
}
impl Drop for Monitor<'_>{fn drop(&mut self){if self.active{sdk::monitor_stop();self.active=false;}}}
/// The exact observedIdleStep interval, including empty/settled idle ticks.
/// All job/descriptor/heap borrows end before the observation is returned.
#[inline(never)]
pub fn collect(clock:fn()->u32,work:impl FnOnce(&Heap<'_>)->R)->(R,Observation){
    let observation=Observation::new(clock());
    let result={
        let mut monitor=Monitor::start(&observation);
        let heap=Heap::new(Some(&observation));
        let result=work(&heap);monitor.finish();result
    };
    observation.finish_time(clock());(result,observation)
}
/// Keep C snprintf's exact bounded capacity and total-length result. Overflow
/// stays unloggable; no truncation, silent trimming or extra output allocation.
struct Record<'a>{bytes:&'a mut[u8],length:usize}
impl<'a> Record<'a>{fn new(bytes:&'a mut[u8])->Self{Self{bytes,length:0}}}
impl Write for Record<'_>{
    fn write_str(&mut self,value:&str)->fmt::Result{
        let room=self.bytes.len().saturating_sub(self.length.saturating_add(1));
        let take=room.min(value.len());
        if take!=0{self.bytes[self.length..self.length+take].copy_from_slice(&value.as_bytes()[..take]);}
        self.length=self.length.saturating_add(value.len());Ok(())
    }
}
fn emit(io:&UiIo,control:&dyn Control,record:Record<'_>)->bool{
    if record.length==0||record.length>=record.bytes.len()||control.stopped()
        ||newt_c3_board::dev_usb::available_for_write()<record.length as i32{return false;}
    sdk::write(io,&record.bytes[..record.length])==record.length
}
#[derive(Default)]
struct Census{scanned:u32,art:u32,no_art:u32,temporary:u32,other:u32,errors:u32,limited:bool,interrupted:bool,skipped:bool}
enum Next{Continue,Stop,End}
#[inline(never)]
fn census(io:&UiIo,control:&dyn Control)->Census{
    let mut out=Census::default();
    if control.stopped(){out.skipped=true;return out;}
    let Ok(mut directory)=sdk::Listing::open(io,super::COVERS)else{out.errors+=1;return out;};
    loop{
        if control.stopped(){out.interrupted=true;break;}
        let next=directory.with_next(|name|{
            let Some(name)=name else{return Next::End;};let name=name.to_bytes();
            if name==b"."||name==b".."{return Next::Continue;}
            if out.scanned==128{out.limited=true;return Next::Stop;}out.scanned+=1;
            if name.ends_with(b".tmp"){out.temporary+=1;return Next::Continue;}
            let root=super::COVERS.to_bytes();let mut path=[0;192];
            let bytes=root.len()+1+name.len();
            if bytes>=path.len(){out.errors+=1;return Next::Continue;}
            path[..root.len()].copy_from_slice(root);path[root.len()]=b'/';
            path[root.len()+1..bytes].copy_from_slice(name);
            if control.stopped(){out.interrupted=true;return Next::Stop;}
            let Ok(path)=core::ffi::CStr::from_bytes_until_nul(&path)else{out.errors+=1;return Next::Continue;};
            match sdk::stat_follow(io,path){
                Err(_)=>out.errors+=1,
                Ok(info) if info.kind!=Kind::Regular=>out.other+=1,
                Ok(info) if info.size==28=>out.no_art+=1,
                Ok(info) if info.size==3412=>out.art+=1,
                Ok(_)=>out.other+=1,
            }
            Next::Continue
        });
        match next{Ok(Next::Continue)=>{},Ok(Next::Stop|Next::End)=>break,Err(_)=>{out.errors+=1;break;}}
    }
    if !directory.close(){out.errors+=1;}out
}
#[inline(never)]
fn report(observation:&Observation,result:R,io:&UiIo,control:&dyn Control)->bool{
    let min_free=if observation.min_free.get()==usize::MAX{0}else{observation.min_free.get()};
    let min_largest=if observation.min_largest.get()==usize::MAX{0}else{observation.min_largest.get()};
    let mut bytes=[0;384];let mut record=Record::new(&mut bytes);
    let formatted=write!(record,"COVER work={} result={} elapsed_ms={} reads={} read_bytes={} writes={} write_bytes={} opens={} path_stats={} min_free_local={} min_largest_observed={} monitor_start={} monitor_stop={} stack_min_free={}\n",
        if result==R::Idle{"cached"}else{"attempt"},result as u8,observation.elapsed.get(),observation.reads.get(),
        observation.read_bytes.get(),observation.writes.get(),observation.write_bytes.get(),observation.opens.get(),
        observation.path_stats.get(),min_free,min_largest,observation.monitor_start.get(),observation.monitor_stop.get(),sdk::stack_min_free());
    if formatted.is_err()||!emit(io,control,record){return false;}
    if result!=R::Idle{
        let census=census(io,control);let mut bytes=[0;160];let mut record=Record::new(&mut bytes);
        let formatted=write!(record,"COVER_CACHE scanned={} art_shape={} noart_shape={} tmp_names={} other={} errors={} limited={} interrupted={} skipped={}\n",
            census.scanned,census.art,census.no_art,census.temporary,census.other,census.errors,
            u8::from(census.limited),u8::from(census.interrupted),u8::from(census.skipped));
        if formatted.is_ok(){emit(io,control,record);}
    }
    true
}

/// An attempt survives cached/empty ticks. A stalled host gets only a bounded
/// drain window before newer attempts can supersede it.
#[derive(Default)]
pub struct Pending(Option<(R,Observation)>);
impl Pending{
    pub fn is_pending(&self)->bool{self.0.is_some()}
    pub fn ready(&self,control:&dyn Control)->bool{
        self.is_pending()&&!control.stopped()&&newt_c3_board::dev_usb::available_for_write()>=200
    }
    pub fn draining(&self,now:u32)->bool{
        // Reuse the completed observation's timestamp; no retry timer or queue.
        self.0.as_ref().is_some_and(|(result,observation)|*result!=R::Idle
            &&now.wrapping_sub(observation.started.wrapping_add(observation.elapsed.get()))<200)
    }
    pub fn record(&mut self,result:R,observation:Observation){
        if result==R::Idle&&self.0.as_ref().is_some_and(|(pending,_)|*pending!=R::Idle){return;}
        if observation.opens.get()!=0||observation.path_stats.get()!=0||result!=R::Idle{
            self.0=Some((result,observation));
        }
    }
    pub fn publish(&mut self,io:&UiIo,control:&dyn Control){
        if !self.ready(control){return;}
        let Some((result,observation))=self.0.as_ref()else{return;};
        if report(observation,*result,io,control){self.0=None;}
    }
}
