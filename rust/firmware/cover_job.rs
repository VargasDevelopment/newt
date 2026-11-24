// SPDX-License-Identifier: MIT
//! Frozen AutomaticLibraryCovers, one ordinary native allocation per idle tick.
use super::{io::{Control,Files},memory::{Memory,ALLOWANCE,RESERVE},session::{self,Session},
    probe::{self,Probe},CoverResult as R,CoverResume};
use core::{cell::Cell,ffi::CStr,mem::size_of,ptr::{self,NonNull},ops::{Deref,DerefMut}};
use newt_native_io::{UiIo,cover as heap};
use super::heap::Heap;
pub struct Targets<'a>{pub names:[Option<&'a CStr>;4],pub count:usize,pub selected:usize,pub view:u32}
impl Default for Targets<'_>{fn default()->Self{Self{names:[None;4],count:0,selected:0,view:0}}}
#[derive(Clone,Copy)]
pub struct Outcome{pub result:R,pub settled:u8,pub slot:u8,pub no_art:bool}
impl Default for Outcome{fn default()->Self{Self{result:R::Idle,settled:0,slot:4,no_art:false}}}
pub struct Job<'a>{
    io:&'a UiIo,control:&'a dyn Control,books:&'a CStr,covers:&'a CStr,
    heap:&'a Heap<'a>,
    pub resume:CoverResume,pub peak:usize,pub live:usize,probe:Probe,
    active:Cell<bool>,stopped:Cell<bool>,heap_declined:Cell<bool>,configured:bool,
}
struct Guard<'a>{outer:&'a dyn Control,stopped:&'a Cell<bool>,heap_declined:&'a Cell<bool>}
impl Control for Guard<'_>{
    fn stopped(&self)->bool{
        if self.stopped.get(){return true;}
        if self.outer.stopped(){self.stopped.set(true);return true;}
        if heap::free_bytes()<RESERVE{self.heap_declined.set(true);self.stopped.set(true);return true;}
        false
    }
    fn yield_now(&self){self.outer.yield_now();}
}
impl Guard<'_>{fn result(&self)->R{if self.heap_declined.get()&&!self.outer.stopped(){R::Declined}else{R::Cancelled}}}
struct Active<'a>(&'a Cell<bool>);
impl Drop for Active<'_>{fn drop(&mut self){self.0.set(false);}}
struct SessionDrop<'a>(*mut Session<'a>);
impl Drop for SessionDrop<'_>{fn drop(&mut self){
    // SAFETY: initialized placed Session, all operation loans returned; this
    // guard runs before the owning Memory block frees its allocation.
    unsafe{ptr::drop_in_place(self.0);}
}}
pub const fn reservation()->usize{size_of::<Job<'_>>()+size_of::<CoverResume>()+2*ALLOWANCE}
pub fn planned_peak()->usize{reservation()+Memory::charge(session::bytes())+Memory::charge(super::member::STAGE_BYTES)}
impl Job<'_>{
    pub fn service(&mut self,targets:&Targets<'_>,settled:u8)->Outcome{
        let mut out=Outcome::default();
        if self.active.get(){out.result=R::Busy;return out;}
        if !self.configured||targets.count>4||(targets.count!=0&&targets.selected>=targets.count){out.settled=15;return out;}
        self.active.set(true);let _active=Active(&self.active);self.stopped.set(false);self.heap_declined.set(false);
        self.peak=0;self.live=0;
        let guarded=Guard{outer:self.control,stopped:&self.stopped,heap_declined:&self.heap_declined};
        for step in 0..targets.count{
            let slot=(targets.selected+step)%targets.count;let bit=1u8<<slot;
            if settled&bit!=0{continue;}
            if !guarded.poll(){out.result=guarded.result();return out;}
            let Some(name)=targets.names[slot]else{out.settled|=bit;continue;};
            if !newt_library_current::valid_library_book_name(name.to_bytes_with_nul()){out.settled|=bit;continue;}
            self.probe=cached(self.io,&guarded,self.heap,self.books,self.covers,name);
            if matches!(self.probe,Probe::Current|Probe::IoError|Probe::RetainedStop){out.settled|=bit;}
            if matches!(self.probe,Probe::Stopped|Probe::RetainedStop)||guarded.stopped(){out.result=guarded.result();return out;}
            if self.probe==Probe::Current{continue;}
            if self.probe!=Probe::Generate{out.result=R::IoError;return out;}
            out.slot=slot as u8;
            let memory=Memory::new(reservation(),self.heap);
            let(result,no_art)={
                let files=Files::new(self.io,self.heap.observation());
                if let Some(mut block)=memory.block(session::bytes()){
                    // SAFETY: actual Session-sized block, scoped references live
                    // through SessionDrop, which precedes the block's release.
                    let pointer=unsafe{Session::initialize(&mut block,&files,&guarded,&memory,&mut self.resume)};
                    let _drop=SessionDrop(pointer);
                    // SAFETY: fully initialized placed Session, exclusively lent.
                    unsafe{(&mut*pointer).run(self.books.to_bytes_with_nul(),self.covers.to_bytes_with_nul(),name)}
                }else{(R::Declined,false)}
            };
            self.peak=memory.peak();self.live=memory.live()-memory.reservation();
            out.no_art=no_art;out.result=if result==R::Cancelled
                ||(self.stopped.get()&&session::verdict(result)&&!no_art){guarded.result()}else{result};
            if out.result!=R::Cancelled{self.resume=CoverResume::default();out.settled|=bit;}
            return out;
        }
        out
    }
}
#[inline(never)]
fn cached(io:&UiIo,control:&dyn Control,heap:&Heap<'_>,books:&CStr,covers:&CStr,name:&CStr)->Probe{
    let mut book=[0;192];let mut thumbnail=[0;192];
    if !newt_library_current::library_book_path(Some(books.to_bytes_with_nul()),Some(name.to_bytes_with_nul()),Some(&mut book))
        ||!newt_library_current::library_name_file(Some(covers.to_bytes_with_nul()),Some(name.to_bytes_with_nul()),Some(b".nlc\0"),Some(&mut thumbnail)){return Probe::IoError;}
    let files=Files::new(io,heap.observation());
    let(Ok(book),Ok(thumbnail))=(CStr::from_bytes_until_nul(&book),CStr::from_bytes_until_nul(&thumbnail))else{return Probe::IoError;};
    probe::probe(&files,control,book,thumbnail)
}
pub struct JobOwner<'a>{pointer:NonNull<Job<'a>>}
impl<'a> JobOwner<'a>{
    pub fn try_new(io:&'a UiIo,control:&'a dyn Control,heap:&'a Heap<'a>,books:&'a CStr,covers:&'a CStr)->Option<Self>{
        // SAFETY: enclosing UI task owns initialized SDK. Native cover allocator
        // enforces the exact frozen alignment/charge/reserve contract.
        let pointer=NonNull::new(unsafe{heap::allocate(size_of::<Job<'a>>())}.cast::<Job<'a>>())?;
        // SAFETY: unique max-aligned actual Job allocation; initialized once.
        unsafe{pointer.as_ptr().write(Job{io,control,books,covers,heap,resume:CoverResume::default(),peak:0,live:0,
            probe:Probe::Generate,active:Cell::new(false),stopped:Cell::new(false),heap_declined:Cell::new(false),
            configured:!books.is_empty()&&books.to_bytes().len()<96&&!covers.is_empty()&&covers.to_bytes().len()<96});}
        Some(Self{pointer})
    }
}
impl<'a>Deref for JobOwner<'a>{type Target=Job<'a>;fn deref(&self)->&Self::Target{
    // SAFETY: initialized uniquely owned actual native Job, live until Drop.
    unsafe{self.pointer.as_ref()}
}}
impl DerefMut for JobOwner<'_>{fn deref_mut(&mut self)->&mut Self::Target{
    // SAFETY: exclusive owner reborrow, no concurrent callback can reenter Job.
    unsafe{self.pointer.as_mut()}
}}
impl Drop for JobOwner<'_>{fn drop(&mut self){
    // SAFETY: all service/Session loans ended; release original Job once through
    // the same cover native allocator after destroying its initialized fields.
    unsafe{self.pointer.as_ptr().drop_in_place();heap::release(self.pointer.as_ptr().cast());}
}}
