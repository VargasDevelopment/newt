// SPDX-License-Identifier: MIT
//! Checked NLC2 probe (cover::record).
use core::ffi::CStr;
use super::io::{Files,Source,Control};
use newt_core::{cover::{self,discovery::le32},zip_window::source::BookSource};
#[derive(Clone,Copy,PartialEq,Eq)]
pub enum Probe{Generate,Current,IoError,Stopped,RetainedStop}
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
            cover::Record::Current=>{retained=true;Probe::Current}
            cover::Record::Generate=>Probe::Generate,
        }
    })();
    if control.stopped(){return if retained{Probe::RetainedStop}else{Probe::Stopped};}
    if files.faulted(){Probe::IoError}else{inspected}
}
