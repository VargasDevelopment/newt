// SPDX-License-Identifier: MIT
//! Frozen transient ZIP staging. Inflation ends before metadata/decode starts.
use super::{io::{self,Files,Source,Control},memory::Memory,CoverResult as R};
use core::{ffi::CStr,mem::{size_of,align_of}};
use newt_core::{cover::{Member,crc},inflate::{State,BitBuffer},
    zip_container::{Reader,InflateScratch,Status},zip_window::source::BookSource};
pub const INFLATE_BYTES: usize = 44*1024;
/// A deflated member's staging block: the inflater, then one write of output.
pub const STAGE_BYTES: usize = INFLATE_BYTES+io::WRITE_BYTES;
pub fn stage(source:&mut Source<'_>,member:&Member,destination:&CStr,
    files:&Files<'_>,control:&dyn Control,memory:&Memory<'_>)->R {
    let inflating=member.entry.method==8;
    let Some(mut block)=memory.block(if inflating {STAGE_BYTES}else{io::WRITE_BYTES}) else { return R::Declined; };
    let (inflater,chunk)=block.as_mut().split_at_mut(if inflating {INFLATE_BYTES}else{0});
    let scratch=if inflating {
        if size_of::<State>()+32768+2048>INFLATE_BYTES || align_of::<State>()>16 { return R::Declined; }
        let pointer=inflater.as_mut_ptr();
        // SAFETY: the block's max-aligned INFLATE_BYTES prefix, exclusively
        // borrowed. State is initialized in its own prefix; dictionary/input
        // are initialized disjoint byte tails. Reader and all scratch loans die
        // before this block is released.
        unsafe {
            let state=pointer.cast::<State>();
            let bytes=core::slice::from_raw_parts_mut(pointer,size_of::<State>());
            if !State::initialize_storage(bytes,BitBuffer::Bits32) { return R::Declined; }
            let dictionary=&mut *pointer.add(size_of::<State>()).cast::<[u8;32768]>();
            let input=&mut *pointer.add(size_of::<State>()+32768).cast::<[u8;2048]>();
            Some(InflateScratch{state:&mut *state,dictionary,input})
        }
    } else { None };
    let mut reader=Reader::new(source,scratch);
    match reader.open(&member.entry) {
        Status::Ok=>{}, Status::OutOfMemory=>return R::Declined,
        Status::IoError=>return R::IoError, _=>return R::Invalid,
    }
    if !control.poll() { return R::Cancelled; }
    let fd=match files.open(destination,true) {
        Ok(fd)=>fd,Err(_)=>{io::remove_temporary(files,destination,None);return R::IoError;}
    };
    let(mut total,mut check,mut result)=(0u64,u32::MAX,R::Ready);
    loop {
        if !control.poll() {result=R::Cancelled;break;}
        let got=reader.read(chunk);
        if got<0 {result=R::Invalid;break;} if got==0 {break;}
        if got as usize>chunk.len() || total+got as u64>u64::from(member.entry.uncompressed_size) {result=R::Invalid;break;}
        check=crc(check,&chunk[..got as usize]);total+=got as u64;
        if !io::write(files,control,fd,&chunk[..got as usize]) {result=R::IoError;break;}
    }
    if result==R::Ready && (total!=u64::from(member.entry.uncompressed_size) || check^u32::MAX!=member.crc32) {result=R::Invalid;}
    if !files.close(fd) {result=R::IoError;}
    if control.stopped() {result=R::Cancelled;}
    if result!=R::Ready {io::remove_temporary(files,destination,None);}
    result
}
pub struct Stored<'a,'b>{pub source:&'a mut Source<'b>,pub offset:u64,pub bytes:u32}
impl BookSource for Stored<'_,'_>{
    fn size(&self)->u64{u64::from(self.bytes)}
    fn read_at(&mut self,at:u64,out:&mut[u8])->i32{
        if at>self.size() || out.len() as u64>self.size()-at{return -1;}
        self.source.read_at(self.offset+at,out)
    }
}
pub fn unsupported(source:&mut dyn BookSource,expected:u32,control:&dyn Control)->(R,bool){
    let mut chunk=[0;256];let mut signature=[0;2];let(mut held,mut at,mut check)=(0,0,u32::MAX);
    while at<source.size(){
        if !control.poll(){return(R::Cancelled,false);}
        let take=chunk.len().min((source.size()-at)as usize);
        let got=source.read_at(at,&mut chunk[..take]);
        if got<=0 || got as usize>take{return(R::IoError,false);}
        for &b in &chunk[..got as usize]{if held<2{signature[held]=b;held+=1;}}
        if held==2 && signature==[255,216]{return(R::Ready,false);}
        check=crc(check,&chunk[..got as usize]);at+=got as u64;
    }
    if source.size()<2 || check^u32::MAX!=expected{(R::Ready,false)}else{(R::Unsupported,true)}
}
