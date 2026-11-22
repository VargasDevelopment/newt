//! Borrowed runtime input/workspace/output seam for root's RV32 retained-code probe.
use super::{Input,Error,Workspace,renderer::{self,Rows,RenderError}};
use crate::page_renderer::{ImageRows,SafetyError};
struct Bytes<'a>{bytes:&'a[u8],pos:usize}
impl Input for Bytes<'_>{fn read(&mut self,out:&mut[u8])->Result<usize,Error>{let n=out.len().min(self.bytes.len()-self.pos);out[..n].copy_from_slice(&self.bytes[self.pos..self.pos+n]);self.pos+=n;Ok(n)}}
struct Memory<'a>{bytes:&'a mut[u8],stride:usize}
impl ImageRows for Memory<'_>{fn on_row(&mut self,y:u16,p:&[u8],w:u16)->Result<bool,SafetyError>{
    if p.len()!=usize::from(w)||self.stride<p.len(){return Err(SafetyError::RowSlice)}
    let start=usize::from(y).checked_mul(self.stride).ok_or(SafetyError::RowSlice)?;
    let end=start.checked_add(p.len()).ok_or(SafetyError::RowSlice)?;
    self.bytes.get_mut(start..end).ok_or(SafetyError::RowSlice)?.copy_from_slice(p);Ok(true)
}}
#[inline(never)]
pub fn jpeg_gray_memory_runtime(bytes:&[u8],work:Workspace<'_>,rows:Rows<'_>,dst:(u16,u16),output:&mut[u8],stride:usize)->Result<(),RenderError>{
    renderer::render(&mut Bytes{bytes,pos:0},work,rows,dst.0,dst.1,&mut Memory{bytes:output,stride})
}
/// First-scan DC preview with caller-owned compressed input, tables and rows.
#[inline(never)]
pub fn jpeg_progressive_memory_runtime(bytes:&[u8],work:super::progressive::Buffers<'_>,dst:(u16,u16),output:&mut[u8],stride:usize)->Result<(),RenderError>{
    super::progressive::render(&mut Bytes{bytes,pos:0},work,dst.0,dst.1,&mut Memory{bytes:output,stride})
}
/// JPEG-only metadata probe; decode callers reopen the source afterward.
pub fn jpeg_info_runtime(bytes:&[u8])->Option<super::detection::Info>{
    super::detection::jpeg(&mut Bytes{bytes,pos:0})
}
