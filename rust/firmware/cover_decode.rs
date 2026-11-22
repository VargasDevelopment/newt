// SPDX-License-Identifier: MIT
//! Actual Core baseline JPEG decoder with frozen arena/CRC/layout/row policy.
use super::{memory::Memory,io::Control,CoverResult as R,member::INFLATE_BYTES};
use core::mem::{size_of,align_of};
use newt_core::{cover::{self,jpeg::{self,Layout}},jpeg::{Input,Error,Workspace,
    renderer::{self,Rows,RenderError}},page_renderer::{ImageRows,SafetyError,BookStatus},
    zip_window::source::BookSource};
#[derive(Default)]
pub struct Drift {pub layout:bool,pub crc:bool}
struct Ahead<'a>{source:&'a mut dyn BookSource,window:&'a mut[u8],held:usize,start:u64}
impl BookSource for Ahead<'_>{
    fn size(&self)->u64{self.source.size()}
    fn read_at(&mut self,at:u64,out:&mut[u8])->i32{
        if at>self.size() || out.len()as u64>self.size()-at{return -1;}
        if out.is_empty(){return 0;}
        if at<self.start || at>=self.start+self.held as u64{
            let take=self.window.len().min((self.size()-at)as usize);
            let got=self.source.read_at(at,&mut self.window[..take]);
            if got<=0{return -1;} self.start=at;self.held=got as usize;
        }
        let take=out.len().min((self.start+self.held as u64-at)as usize);
        out[..take].copy_from_slice(&self.window[(at-self.start)as usize..(at-self.start)as usize+take]);take as i32
    }
}
struct Checked<'a>{source:&'a mut dyn BookSource,layout:&'a Layout,at:u64,crc:u32,changed:bool,io_error:bool}
impl Input for Checked<'_>{
    fn read(&mut self,out:&mut[u8])->Result<usize,Error>{
        if self.changed{return Err(Error::Input);}
        let take=out.len().min(self.source.size().saturating_sub(self.at)as usize);
        if take==0{return Ok(0);}
        let got=self.source.read_at(self.at,&mut out[..take]);
        if got<=0 || got as usize>take{self.io_error=true;return Err(Error::Input);}
        let got=got as usize;
        if !self.layout.matches(self.at,&out[..got]){self.changed=true;return Err(Error::Input);}
        self.crc=cover::crc(self.crc,&out[..got]);self.at+=got as u64;Ok(got)
    }
}
impl Checked<'_>{
    fn finish(&mut self,expected:u32,scratch:&mut[u8])->bool{
        while !self.changed && self.at<self.source.size(){
            let take=scratch.len().min((self.source.size()-self.at)as usize);
            if !self.read(&mut scratch[..take]).is_ok_and(|n|n!=0){return false;}
        }
        !self.changed && self.at==self.source.size() && self.crc^u32::MAX==expected
    }
}
struct Raster<'a>{control:&'a dyn Control,pixels:&'a mut[u8],width:u16,height:u16,left:usize,top:usize,rows:u16,cancelled:bool,invalid:bool}
impl ImageRows for Raster<'_>{
    fn on_row(&mut self,y:u16,gray:&[u8],width:u16)->Result<bool,SafetyError>{
        if !self.control.poll(){self.cancelled=true;return Ok(false);}
        if y!=self.rows || y>=self.height || width!=self.width{self.invalid=true;return Ok(false);}
        let mut bits=[0;18];jpeg::dither_row(&gray[..usize::from(width)],usize::from(y)+self.top,&mut bits);
        let out=&mut self.pixels[(usize::from(y)+self.top)*18..];
        for x in 0..usize::from(width){if bits[x/8]&(0x80>>(x%8))!=0{let target=x+self.left;out[target/8]|=0x80>>(target%8);}}
        self.rows+=1;Ok(true)
    }
}
struct Parts<'a>(&'a mut[u8]);
impl<'a> Parts<'a>{
    fn bytes(&mut self,n:usize)->Option<&'a mut[u8]>{
        if n>self.0.len(){return None;}
        let (a,b)=core::mem::take(&mut self.0).split_at_mut(n);self.0=b;Some(a)
    }
    /// # Safety
    /// T is a numeric all-zero-valid type, and the backing's bytes are initialized.
    unsafe fn numeric<T>(&mut self,n:usize)->Option<&'a mut[T]>{
        let pad=self.0.as_ptr().align_offset(align_of::<T>());self.bytes(pad)?;
        let bytes=self.bytes(size_of::<T>().checked_mul(n)?)?;
        // SAFETY: private callers use only i32/i16/u32/u16 with initialized zero
        // bytes; checked aligned disjoint prefix, exact element count.
        Some(unsafe{core::slice::from_raw_parts_mut(bytes.as_mut_ptr().cast(),n)})
    }
}
pub fn decode(source:&mut dyn BookSource,expected:u32,memory:&Memory<'_>,control:&dyn Control,pixels:&mut[u8],drift:&mut Drift)->R{
    if !control.poll(){return R::Cancelled;}
    let Some(mut layout_block)=memory.block(size_of::<Layout>())else{return R::Declined;};
    // SAFETY: actual max-aligned Layout-sized exclusive allocation, initialized
    // before reference formation; scalar/array-only Layout has no destructor.
    let layout=unsafe{let p=layout_block.pointer().cast::<Layout>();p.write(Layout::default());&mut*p};
    let baseline=jpeg::baseline(source,layout);
    if baseline!=jpeg::Status::Ready && control.stopped(){return R::Cancelled;}
    match baseline{
        jpeg::Status::Ready=>{},jpeg::Status::Unsupported=>return R::Unsupported,
        jpeg::Status::Incomplete=>return R::Incomplete,jpeg::Status::Invalid=>return R::Invalid,
        jpeg::Status::IoError=>return R::IoError,
    }
    let mut end=[0;2];
    let bytes=source.size();
    if !cover::read(source,bytes-2,&mut end){return if control.stopped(){R::Cancelled}else{R::IoError};}
    if end!=[255,217]{return R::Incomplete;}
    let target=jpeg::placement(layout.width,layout.height);
    let arena_bytes=jpeg::arena_bytes((layout.width,layout.height),target);
    let object=(size_of::<Ahead<'_>>()+15)&!15;
    let ahead=if Memory::charge(size_of::<Layout>())+Memory::charge(cover::PIXELS)+Memory::charge(object+4096+arena_bytes)
        <=Memory::charge(INFLATE_BYTES){object+4096}else{0};
    let Some(mut scratch)=memory.block(ahead+arena_bytes)else{return R::Declined;};
    let (front,arena)=scratch.as_mut().split_at_mut(ahead);
    let input:&mut dyn BookSource=if ahead==0{source}else{
        let (place,window)=front.split_at_mut(object);
        // SAFETY: checked max-aligned object prefix, actual read-ahead view and
        // disjoint initialized window. Its borrowed source never escapes decode.
        unsafe{let p=place.as_mut_ptr().cast::<Ahead<'_>>();p.write(Ahead{source,window,held:0,start:0});&mut*p}
    };
    let scaled=usize::from(layout.width>>jpeg::scale((layout.width,layout.height),target));
    let mut parts=Parts(arena);let Some(skip)=parts.bytes(1024)else{return R::Declined;};
    let Some(pool)=parts.bytes(8192)else{return R::Declined;};let mut pool=Parts(pool);
    // SAFETY: numeric zero-valid initialized arrays withdrawn disjointly from
    // the original 8KiB JPEG pool. No decoder allocation occurs outside it.
    let Some((workspace,rows))=(unsafe{(||{
        let workspace=Workspace{input:pool.bytes(512)?,segment:pool.bytes(512)?,quant:pool.numeric::<i32>(256)?,
            huffman:pool.bytes(1088)?,block:pool.numeric::<i32>(64)?,mcu:pool.numeric::<i16>(384)?,pixels:pool.bytes(256)?};
        let rows=Rows{band:parts.bytes(16*scaled)?,row:parts.bytes(usize::from(target.0))?,
            sums:parts.numeric::<u32>(usize::from(target.0))?,counts:parts.numeric::<u16>(usize::from(target.0))?};
        Some((workspace,rows))
    })()})else{return R::Declined;};
    pixels.fill(0);
    let mut raster=Raster{control,pixels,width:target.0,height:target.1,left:usize::from(144-target.0)/2,
        top:usize::from(188-target.1)/2,rows:0,cancelled:false,invalid:false};
    let mut checked=Checked{source:input,layout,at:0,crc:u32::MAX,changed:false,io_error:false};
    let result=renderer::render(&mut checked,workspace,rows,target.0,target.1,&mut raster);
    if raster.cancelled || control.stopped(){return R::Cancelled;}
    if checked.changed{drift.layout=true;return R::Invalid;}
    if matches!(result,Err(RenderError::Status(BookStatus::OutOfMemory))){return R::Declined;}
    if checked.io_error{return R::IoError;}
    if matches!(result,Err(RenderError::Status(BookStatus::Truncated))) || (result.is_ok()&&raster.rows!=target.1){return R::Incomplete;}
    if result.is_err() || raster.invalid{return if control.stopped(){R::Cancelled}else{R::Invalid};}
    if !checked.finish(expected,skip){drift.layout=checked.changed;drift.crc=!checked.changed;
        return if control.stopped(){R::Cancelled}else{R::Invalid};}
    R::Ready
}
