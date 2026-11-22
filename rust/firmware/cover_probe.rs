// SPDX-License-Identifier: MIT
//! Checked NLC2 probe (cover::record) and complete CJB3 legacy-flag migration walk.
use core::ffi::CStr;
use super::io::{Files,Source,Control};
use newt_core::{cover::{self,discovery::{self,le16,le32},MetadataStatus as S},zip_window::source::BookSource};
#[derive(Clone,Copy,PartialEq,Eq)]
pub enum Probe{Generate,Current,IoError,Stopped,RetainedStop}
enum Migration{Qualifying,Unchanged,Inconclusive}
fn migration(files:&Files<'_>,control:&dyn Control,path:&CStr)->Migration{
    let mut source=Source::new(files,control);
    if !source.open(path,64*1024*1024){return Migration::Inconclusive;}
    if source.size()<22{return Migration::Unchanged;}
    let d=match discovery::directory(&mut source){Ok(d)=>d,Err(S::IoError)=>return Migration::Inconclusive,_=>return Migration::Unchanged};
    if d.count==0||d.count>1024||d.bytes>1024*1024{return Migration::Unchanged;}
    let(mut at,mut expanded,mut qualifying)=(u64::from(d.offset),0,false);let end=at+u64::from(d.bytes);
    for _ in 0..d.count{
        let mut header=[0;46];let mut name=[0;512];
        let next=match discovery::directory_member(&mut source,at,end,d.offset,&mut expanded,&mut header,&mut name){
            Ok((next,_))=>next,Err(S::IoError)=>return Migration::Inconclusive,_=>return Migration::Unchanged};
        qualifying|=le16(&header[8..])&!0x080e!=0;at=next;
    }
    if at!=end{return Migration::Unchanged;}
    if !source.still_bound(path)||!source.close(){return Migration::Inconclusive;}
    if qualifying{Migration::Qualifying}else{Migration::Unchanged}
}
pub fn probe(files:&Files<'_>,control:&dyn Control,book:&CStr,thumbnail:&CStr)->Probe{
    let mut retained=false;
    let inspected=(||{
        let mut file=Source::new(files,control);
        if !control.poll(){return Probe::Stopped;}
        let Ok(source)=files.stat(book)else{return Probe::IoError;};
        if !source.regular()||source.size<=0||source.seconds<0||source.size>64*1024*1024{return Probe::IoError;}
        if !file.open_cache(thumbnail,(28+cover::PIXELS)as u64){return Probe::Generate;}
        if file.size()!=28&&file.size()!=(28+cover::PIXELS)as u64{return Probe::Generate;}
        let payload=file.size()as usize-28;let mut header=[0;28];
        if !cover::read(&mut file,0,&mut header){return Probe::IoError;}
        let Ok(size)=<[u8;8]>::try_from(&header[8..16])else{return Probe::IoError;};
        let Ok(mtime)=<[u8;8]>::try_from(&header[16..24])else{return Probe::IoError;};
        if header[..8]!=cover::SIGNATURE||u64::from_le_bytes(size)!=source.size as u64
            ||u64::from_le_bytes(mtime)!=(source.seconds as u64&!1){return Probe::Generate;}
        let mut bytes=[0;256];let mut crc=u32::MAX;
        for at in(0..payload).step_by(256){let take=256.min(payload-at);
            if !cover::read(&mut file,28+at as u64,&mut bytes[..take]){return Probe::IoError;}
            crc=cover::crc(crc,&bytes[..take]);}
        match cover::record((payload!=0).then_some(crc^u32::MAX),le32(&header[24..])){
            cover::Record::Migrate=>{
                retained=true;if !file.close(){return Probe::IoError;}
                match migration(files,control,book){Migration::Qualifying=>{retained=false;Probe::Generate},
                    Migration::Unchanged=>Probe::Current,Migration::Inconclusive=>Probe::IoError}
            }
            cover::Record::Current=>{retained=true;Probe::Current}
            cover::Record::Generate=>Probe::Generate,
        }
    })();
    if control.stopped(){return if retained{Probe::RetainedStop}else{Probe::Stopped};}
    if files.faulted(){Probe::IoError}else{inspected}
}
