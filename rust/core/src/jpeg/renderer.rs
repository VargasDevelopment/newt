//! Baseline ImageRenderer policy. Uses the core's shared RowScaler.
use super::{Decoder,Error,Input,Output,Rect,Workspace};
use crate::{image_renderer::RowScaler,page_renderer::{BookStatus,ImageRows,SafetyError}};
pub struct Rows<'a> {
    pub band:&'a mut[u8],pub row:&'a mut[u8],pub sums:&'a mut[u32],pub counts:&'a mut[u16],
}
struct Band<'a,'b> { scaler:RowScaler<'a>,band:&'a mut[u8],top:u32,filled:u32,width:usize,sink:&'b mut dyn ImageRows,error:Option<SafetyError> }
impl Band<'_,'_> {
    fn flush(&mut self,up:u32) {
        while self.top<up&&!self.scaler.stopped {
            let p=(self.top as usize%16)*self.width;
            match self.scaler.feed(self.top,&self.band[p..p+self.width],self.sink) {Ok(_)=>{},Err(e)=>{self.error=Some(e);return}}
            self.top+=1;
        }
    }
}
impl Output for Band<'_,'_> {
    fn write(&mut self,r:Rect,p:&[u8])->bool {
        if self.scaler.stopped||self.error.is_some(){return false}
        if u32::from(r.top)>=self.filled {self.flush(self.filled);self.filled=u32::from(r.bottom)+1;}
        let rw=usize::from(r.right-r.left)+1;
        for y in r.top..=r.bottom {let dst=usize::from(y)%16*self.width+usize::from(r.left);let src=usize::from(y-r.top)*rw;self.band[dst..dst+rw].copy_from_slice(&p[src..src+rw]);}
        // Native writes this MCU even when flush stopped, then stops on the next call.
        self.error.is_none()
    }
}
#[derive(Debug)]
pub enum RenderError { Status(BookStatus),Safety(SafetyError) }
/// Input errors must retain IoError externally when adapting a ZipEntryReader;
/// this entry point maps unsupported headers to Unsupported and other decoder errors to ParseError.
pub fn render(input:&mut dyn Input,w:Workspace<'_>,rows:Rows<'_>,width:u16,height:u16,sink:&mut dyn ImageRows)->core::result::Result<(),RenderError> {
    let mut d=Decoder::prepare(w,input).map_err(|e|RenderError::Status(if e==Error::Unsupported{BookStatus::Unsupported}else{BookStatus::ParseError}))?;
    let mut scale=0;
    while scale<3&&(d.width>>(scale+1))>=width&&(d.height>>(scale+1))>=height{scale+=1;}
    let sw=d.width>>scale;let sh=d.height>>scale;
    if sw==0||sh==0{return Err(RenderError::Status(BookStatus::Unsupported))}
    if rows.band.len()<usize::from(sw)*16{return Err(RenderError::Status(BookStatus::OutOfMemory))}
    let scaler=RowScaler::new(sw,sh,width.min(sw),height.min(sh),rows.row,rows.sums,rows.counts).map_err(RenderError::Status)?;
    let mut b=Band{scaler,band:rows.band,top:0,filled:0,width:usize::from(sw),sink,error:None};
    let result=d.decode(input,&mut b,scale);
    if let Some(e)=b.error{return Err(RenderError::Safety(e))}
    if result.is_err()&&!b.scaler.stopped{return Err(RenderError::Status(BookStatus::ParseError))}
    if !b.scaler.stopped{b.flush(u32::from(sh));}
    if let Some(e)=b.error{return Err(RenderError::Safety(e))}
    // JDR_INTR after a requested row stop is successful in the original renderer.
    let _=Error::Interrupted;
    Ok(())
}
