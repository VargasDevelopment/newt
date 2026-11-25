// SPDX-License-Identifier: MIT
//! Frozen cover_job Session: one bound version and transactional temporaries.
use super::{io::{self,Files,Source,Control},memory::{Memory,Block},member::{self,Stored},
    decode::{self,Drift},xml,CoverResult as R,CoverResume,Step};
use core::{ffi::CStr,mem::size_of,ptr};
use newt_core::{cover::{self,Member,MetadataStatus as M,discovery,xml::Found},member_check::CheckedRecord,zip_window::source::BookSource};
const STAGE:u8=1;const PENDING:u8=2;const EARLY:u8=4;
#[derive(Clone,Copy)]
enum Document{Container,Package,Page}
fn reason(result:R)->Option<cover::NoCoverReason>{match result{
    R::Missing=>Some(cover::NoCoverReason::Missing),
    R::Unsupported=>Some(cover::NoCoverReason::Unsupported),
    R::Incomplete=>Some(cover::NoCoverReason::Incomplete),
    R::Invalid=>Some(cover::NoCoverReason::Invalid),
    _=>None,
}}
pub fn verdict(result:R)->bool{reason(result).is_some()}
pub fn metadata(result:M)->R{match result{M::Ok=>R::Ready,M::NotFound=>R::Missing,M::Unsupported=>R::Unsupported,
    M::IoError=>R::IoError,M::OutOfMemory=>R::Declined,_=>R::Invalid}}
macro_rules! path {
    ($bytes:expr,$failure:expr)=>{
        match CStr::from_bytes_until_nul($bytes){Ok(path)=>path,Err(_)=>return $failure}
    };
}
pub struct Session<'a>{
    files:&'a Files<'a>,control:&'a dyn Control,memory:&'a Memory<'a>,resume:&'a mut CoverResume,
    source:Source<'a>,book:[u8;192],stage:[u8;192],pending:[u8;192],final_path:[u8;192],
    opf:[u8;512],found:Found,member:Member,source_crc:u32,
    faulted:bool,publishing:bool,no_art:bool,temporaries:u8,
}
impl<'a> Session<'a>{
    /// # Safety
    /// block is a unique max-aligned Session-sized allocation. All references
    /// outlive the placed Session, whose destructor must run before block frees.
    pub unsafe fn initialize(block:&mut Block<'_>,files:&'a Files<'a>,control:&'a dyn Control,
        memory:&'a Memory<'a>,resume:&'a mut CoverResume)->*mut Self{
        let p=block.pointer().cast::<Self>();
        // SAFETY: each actual field is initialized directly in its final owner;
        // no giant Session/path arrays are constructed on the call stack.
        unsafe{
            ptr::addr_of_mut!((*p).files).write(files);ptr::addr_of_mut!((*p).control).write(control);
            ptr::addr_of_mut!((*p).memory).write(memory);ptr::addr_of_mut!((*p).resume).write(resume);
            ptr::addr_of_mut!((*p).source).write(Source::new(files,control));
            ptr::addr_of_mut!((*p).book).cast::<u8>().write_bytes(0,192);
            ptr::addr_of_mut!((*p).stage).cast::<u8>().write_bytes(0,192);
            ptr::addr_of_mut!((*p).pending).cast::<u8>().write_bytes(0,192);
            ptr::addr_of_mut!((*p).final_path).cast::<u8>().write_bytes(0,192);
            ptr::addr_of_mut!((*p).opf).cast::<u8>().write_bytes(0,512);
            // All-zero Found is empty: no image and no page.
            ptr::addr_of_mut!((*p).found).write_bytes(0,1);
            ptr::addr_of_mut!((*p).member).write(Member::default());ptr::addr_of_mut!((*p).source_crc).write(0);
            ptr::addr_of_mut!((*p).faulted).write(false);ptr::addr_of_mut!((*p).publishing).write(false);
            ptr::addr_of_mut!((*p).no_art).write(false);ptr::addr_of_mut!((*p).temporaries).write(0);
        }
        p
    }
    pub fn run(&mut self,books:&[u8],covers:&[u8],name:&CStr)->(R,bool){
        let result=self.attempt(books,covers,name);
        if !self.source.bound(){self.resume.book=0;}
        let Some(reason)=reason(result)else{self.no_art=false;return(result,false);};
        if !self.unfaulted(){self.no_art=false;return(result,false);}
        self.no_art=if self.temporaries&EARLY!=0{self.record_early(reason)}else{
            self.resume.step=Step::Decided;self.resume.verdict=result;self.record_no_art(reason)
        };
        (result,self.no_art)
    }
    fn cancelled(&self,result:R)->R{if self.control.stopped(){R::Cancelled}else{result}}
    fn unfaulted(&self)->bool{!self.publishing&&!self.faulted&&!self.files.faulted()&&!self.source.faulted()&&self.memory.rejected()==0}
    fn paths(&mut self,books:&[u8],covers:&[u8],name:&CStr)->bool{
        newt_library_current::library_book_path(Some(books),Some(name.to_bytes_with_nul()),Some(&mut self.book))
            &&newt_library_current::library_name_file(Some(covers),Some(name.to_bytes_with_nul()),Some(b".cover-stage.tmp\0"),Some(&mut self.stage))
            &&newt_library_current::library_name_file(Some(covers),Some(name.to_bytes_with_nul()),Some(b".nlc.tmp\0"),Some(&mut self.pending))
            &&newt_library_current::library_name_file(Some(covers),Some(name.to_bytes_with_nul()),Some(b".nlc\0"),Some(&mut self.final_path))
    }
    fn ensure_directory(&self,directory:&CStr)->bool{
        if !self.control.poll(){return false;}
        match self.files.stat(directory){Ok(info)=>info.directory(),Err(newt_native_storage::Error::Missing)=>
            self.control.poll()&&self.files.mkdir(directory),_=>false}
    }
    fn attempt(&mut self,books:&[u8],covers:&[u8],name:&CStr)->R{
        if !self.paths(books,covers,name)||!self.ensure_directory(path!(covers,R::IoError)){return self.cancelled(R::IoError);}
        if !io::remove_temporary(self.files,path!(&self.stage,R::IoError),Some(self.control))
            ||!io::remove_temporary(self.files,path!(&self.pending,R::IoError),Some(self.control)){return self.cancelled(R::IoError);}
        if !self.source.open(path!(&self.book,R::IoError),64*1024*1024){return self.cancelled(R::IoError);}
        let binding=self.source.binding();let key=cover::version_key(name.to_bytes(),binding.size as u64,binding.seconds as u64);
        if self.resume.book!=key{*self.resume=CoverResume::default();}self.resume.book=key;
        if self.resume.step==Step::Empty&&self.source.size()>=8*1024*1024{
            let(result,rejected)=self.early_unsupported();
            if matches!(result,R::Cancelled|R::IoError|R::Declined){return self.cancelled(result);}
            if rejected&&result==R::Unsupported{self.temporaries|=EARLY;return result;}
        }
        if self.resume.step==Step::Empty{
            let Some(mut block)=self.memory.block(io::CRC_BYTES)else{return R::Declined;};
            let Some(crc)=self.source.crc(block.as_mut())else{return self.cancelled(R::IoError);};
            self.resume.source_crc=crc;self.resume.step=Step::Read;
        }
        self.source_crc=self.resume.source_crc;
        if self.resume.step==Step::Decided{return self.resume.verdict;}
        if self.resume.step==Step::Located{
            self.member=Member{entry:self.resume.member.member(),
                crc32:self.resume.crc32,data:u64::from(self.resume.data)};
        }else{let result=self.locate(true);if result!=R::Ready{return self.cancelled(result);}}
        if self.member.entry.method!=0{
            let result=member::stage(&mut self.source,&self.member,path!(&self.stage,R::IoError),self.files,self.control,self.memory);
            if result!=R::Ready{return self.cancelled(result);}self.temporaries|=STAGE;
        }
        let Some(mut raster)=self.memory.block(cover::PIXELS)else{return R::Declined;};
        let mut drift=Drift::default();
        let result=if self.member.entry.method==0{
            let mut raw=Stored{source:&mut self.source,offset:self.member.data,bytes:self.member.entry.uncompressed_size};
            let result=decode::decode(&mut raw,self.member.crc32,self.memory,self.control,raster.as_mut(),&mut drift);
            self.faulted|=drift.layout;result
        }else{
            let mut raw=Source::new(self.files,self.control);
            if !raw.open(path!(&self.stage,R::IoError),4*1024*1024){return self.cancelled(R::IoError);}
            let mut result=decode::decode(&mut raw,self.member.crc32,self.memory,self.control,raster.as_mut(),&mut drift);
            if !raw.valid(){result=R::Invalid;}self.faulted|=drift.layout||drift.crc||raw.faulted();result
        };
        if result!=R::Ready{return self.cancelled(result);}
        if self.temporaries&STAGE!=0{
            if !io::remove_temporary(self.files,path!(&self.stage,R::IoError),Some(self.control)){return self.cancelled(R::IoError);}
            self.temporaries&=!STAGE;
        }
        if !self.source.valid(){return self.cancelled(R::Invalid);}self.publishing=true;
        self.publish(raster.as_mut())
    }
    fn xml(&mut self,document:Document)->R{
        let name=match document{Document::Container=>b"META-INF/container.xml".as_slice(),
            Document::Package=>discovery::cbytes(&self.opf),Document::Page=>discovery::cbytes(&self.found.page)};
        let status=discovery::locate(&mut self.source,name,&mut self.member);
        if status!=M::Ok{return metadata(status);}
        if self.member.entry.uncompressed_size>256*1024{return R::Unsupported;}
        let result=member::stage(&mut self.source,&self.member,path!(&self.stage,R::IoError),self.files,self.control,self.memory);
        if result!=R::Ready{return result;}self.temporaries|=STAGE;
        let status={let mut raw=Source::new(self.files,self.control);
            if !raw.open(path!(&self.stage,R::IoError),256*1024){return R::IoError;}
            let io=self.files.io();
            let status=match document{Document::Container=>xml::container(io,&mut raw,self.memory,&mut self.found),
                Document::Package=>xml::package(io,&mut raw,self.memory,&self.opf,&mut self.found),
                Document::Page=>xml::page(io,&mut raw,self.memory,&mut self.found)};
            self.faulted|=raw.faulted();status};
        if !io::remove_temporary(self.files,path!(&self.stage,R::IoError),Some(self.control)){return self.cancelled(R::IoError);}
        self.temporaries&=!STAGE;metadata(status)
    }
    fn locate(&mut self,remember:bool)->R{
        let result=self.xml(Document::Container);if result!=R::Ready{return result;}
        self.opf=self.found.image;
        let result=self.xml(Document::Package);if result!=R::Ready{return result;}
        if self.found.page[0]!=0{
            // A cover page that gives no image falls back to the best image.
            let result=self.xml(Document::Page);
            if result!=R::Ready&&(!verdict(result)||self.found.image[0]==0){return result;}
        }
        let result=metadata(discovery::locate(&mut self.source,&self.found.image,&mut self.member));
        if remember&&result==R::Ready&&self.unfaulted(){
            self.resume.member=CheckedRecord::from(&self.member.entry);self.resume.crc32=self.member.crc32;
            self.resume.data=self.member.data as u32;self.resume.step=Step::Located;
        }result
    }
    fn early_unsupported(&mut self)->(R,bool){
        let result=self.locate(false);if result!=R::Ready{return(result,result==R::Unsupported);}
        if self.member.entry.method!=0{
            let result=member::stage(&mut self.source,&self.member,path!(&self.stage,(R::IoError,false)),self.files,self.control,self.memory);
            if result!=R::Ready{return(result,false);}self.temporaries|=STAGE;
            let(mut result,rejected)={let mut raw=Source::new(self.files,self.control);
                if !raw.open(path!(&self.stage,(R::IoError,false)),4*1024*1024){(R::IoError,false)}else{
                    let(result,rejected)=member::unsupported(&mut raw,self.member.crc32,self.control);
                    (if raw.valid(){result}else{R::IoError},rejected)}};
            if !io::remove_temporary(self.files,path!(&self.stage,(R::IoError,false)),Some(self.control)){result=self.cancelled(R::IoError);}
            else{self.temporaries&=!STAGE;}(result,rejected)
        }else{
            let mut raw=Stored{source:&mut self.source,offset:self.member.data,bytes:self.member.entry.uncompressed_size};
            let(result,rejected)=member::unsupported(&mut raw,self.member.crc32,self.control);
            (if self.source.valid(){result}else{R::IoError},rejected)
        }
    }
    fn header(&self,check:u32)->[u8;28]{cover::header(self.source.size(),self.source.binding().seconds as u64,check)}
    fn record_no_art(&mut self,reason:cover::NoCoverReason)->bool{
        self.control.poll()&&self.source.still_bound(path!(&self.book,false))&&self.commit(&self.header(cover::no_art_route(reason)),&[])==R::Produced
    }
    fn publish(&mut self,pixels:&[u8])->R{
        if !self.control.poll(){return R::Cancelled;}
        if !self.source.still_bound(path!(&self.book,R::IoError)){return self.cancelled(R::Invalid);}
        self.commit(&self.header(cover::crc(u32::MAX,pixels)^u32::MAX),pixels)
    }
    fn verify_pending(&self,header:&[u8;28],payload:&[u8])->bool{
        let mut file=Source::new(self.files,self.control);
        if !file.open(path!(&self.pending,false),28+cover::PIXELS as u64)||file.size()!=28+payload.len()as u64{return false;}
        let mut bytes=[0;256];
        if !cover::read(&mut file,0,&mut bytes[..28])||bytes[..28]!=*header{return false;}
        for at in (0..payload.len()).step_by(256){let take=256.min(payload.len()-at);
            if !cover::read(&mut file,28+at as u64,&mut bytes[..take])||bytes[..take]!=payload[at..at+take]{return false;}}
        file.still_bound(path!(&self.pending,false))
    }
    fn commit(&mut self,header:&[u8;28],payload:&[u8])->R{
        if !self.control.poll(){return R::Cancelled;}
        // Admit all commit workspace before writing a pending record.
        let Some(mut block)=self.memory.block(io::CRC_BYTES)else{return R::Declined;};
        self.temporaries|=PENDING;
        let Ok(fd)=self.files.open(path!(&self.pending,R::IoError),true)else{return R::IoError;};
        let mut valid=io::write(self.files,self.control,fd,header)
            &&(payload.is_empty()||io::write(self.files,self.control,fd,payload))&&self.control.poll()&&self.files.sync(fd);
        if !self.files.close(fd){valid=false;}
        if !valid||!self.verify_pending(header,payload)||!self.source.still_bound(path!(&self.book,R::IoError))||!self.control.poll(){return self.cancelled(R::IoError);}
        if self.source.crc(block.as_mut())!=Some(self.source_crc)||!self.source.still_bound(path!(&self.book,R::IoError)){return self.cancelled(R::Invalid);}
        if !self.current_path_matches(block.as_mut()){return self.cancelled(R::Invalid);}
        if !self.prepare_final(){return self.cancelled(R::IoError);}
        if !self.source.still_bound(path!(&self.book,R::IoError)){return self.cancelled(R::Invalid);}
        if !self.control.poll(){return R::Cancelled;}
        if !self.files.rename(path!(&self.pending,R::IoError),path!(&self.final_path,R::IoError)){return R::IoError;}
        self.temporaries&=!PENDING;R::Produced
    }
    fn current_path_matches(&self,block:&mut[u8])->bool{
        if self.files.complete_handle_metadata(){return true;}
        let mut current=Source::new(self.files,self.control);
        let matches=current.open(path!(&self.book,false),64*1024*1024)&&self.source.binding().same(current.binding())
            &&current.crc(block)==Some(self.source_crc)&&current.still_bound(path!(&self.book,false));
        current.close()&&matches
    }
    fn prepare_final(&self)->bool{
        if !self.control.poll(){return false;}
        let existing=match self.files.stat(path!(&self.final_path,false)){
            Err(newt_native_storage::Error::Missing)=>return true,Ok(s)=>s,_=>return false};
        if !existing.regular()||existing.links>1||existing.size<8||existing.size>28+cover::PIXELS as i64{return false;}
        let mut old=Source::new(self.files,self.control);let mut signature=[0;8];
        let recognized=old.open(path!(&self.final_path,false),28+cover::PIXELS as u64)
            &&cover::read(&mut old,0,&mut signature)&&signature==cover::SIGNATURE&&old.still_bound(path!(&self.final_path,false));
        if !old.close()||!recognized||!self.control.poll(){return false;}
        if !self.control.poll()||!self.files.stat(path!(&self.final_path,false)).is_ok_and(|s|existing.same(s)){return false;}
        // Actual pinned FAT rename refuses an existing destination.
        self.files.rename_replaces_destination()
            ||(self.control.poll()&&self.files.unlink(path!(&self.final_path,false)).is_ok())
    }
    fn record_early(&mut self,reason:cover::NoCoverReason)->bool{
        if !self.control.poll()||!self.source.still_bound(path!(&self.book,false)){return false;}
        let header=self.header(cover::no_art_route(reason));self.temporaries|=PENDING;
        let Ok(fd)=self.files.open(path!(&self.pending,false),true)else{return false;};
        let mut valid=io::write(self.files,self.control,fd,&header)&&self.control.poll()&&self.files.sync(fd);
        if !self.files.close(fd){valid=false;}
        if !valid||!self.verify_pending(&header,&[])||!self.control.poll(){return false;}
        let binding=self.source.binding();self.source.close();
        if !self.source.open(path!(&self.book,false),64*1024*1024)||!binding.same(self.source.binding()){return false;}
        let(current,rejected)=self.early_unsupported();
        if current!=R::Unsupported||!rejected||!self.unfaulted()||!self.source.still_bound(path!(&self.book,false))||!self.control.poll(){return false;}
        self.publishing=true;
        if !self.prepare_final()||!self.source.still_bound(path!(&self.book,false))||!self.control.poll()
            ||!self.files.rename(path!(&self.pending,false),path!(&self.final_path,false)){return false;}
        self.temporaries&=!PENDING;true
    }
}
impl Drop for Session<'_>{
    fn drop(&mut self){
        self.source.close();
        if self.temporaries&STAGE!=0{if let Ok(path)=CStr::from_bytes_until_nul(&self.stage){io::remove_temporary(self.files,path,None);}}
        if self.temporaries&PENDING!=0{if let Ok(path)=CStr::from_bytes_until_nul(&self.pending){io::remove_temporary(self.files,path,None);}}
    }
}
pub const fn bytes()->usize{size_of::<Session<'_>>()}
// The commit's CRC block, beside the raster, stays below the staging block.
const _:()=assert!(Memory::charge(cover::PIXELS)+Memory::charge(io::CRC_BYTES)<=Memory::charge(member::STAGE_BYTES));
