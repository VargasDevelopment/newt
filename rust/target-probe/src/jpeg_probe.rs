//! Minimal unsafe ABI adapter; decoder/policy remain safe Rust. No allocator.
use newt_core::jpeg::{Error,Input,Workspace,renderer::{self,RenderError,Rows},progressive::{self,Huffman}};
use newt_core::page_renderer::{ImageRows,SafetyError};
#[repr(C)]
pub struct Span<T>{ptr:*mut T,len:usize}
#[repr(C)]
pub struct Runtime {
    encoded:Span<u8>,input:Span<u8>,segment:Span<u8>,quant:Span<i32>,huffman:Span<u8>,block:Span<i32>,mcu:Span<i16>,pixels:Span<u8>,
    band:Span<u8>,row:Span<u8>,sums:Span<u32>,counts:Span<u16>,output:Span<u8>,stride:usize,width:u16,height:u16,
}
impl<T> Span<T>{
    // Caller owns all ranges, disjoint for the entire call, valid and aligned.
    unsafe fn borrow(&mut self)->Option<&mut[T]>{if self.ptr.is_null()||(self.ptr as usize)%core::mem::align_of::<T>()!=0||self.len>isize::MAX as usize/core::mem::size_of::<T>(){None}else{Some(unsafe{core::slice::from_raw_parts_mut(self.ptr,self.len)})}}
}
struct Bytes<'a>{data:&'a[u8],pos:usize}
impl Input for Bytes<'_>{fn read(&mut self,dst:&mut[u8])->Result<usize,Error>{let n=dst.len().min(self.data.len()-self.pos);dst[..n].copy_from_slice(&self.data[self.pos..self.pos+n]);self.pos+=n;Ok(n)}}
struct Sink<'a>{data:&'a mut[u8],stride:usize}
impl ImageRows for Sink<'_>{fn on_row(&mut self,y:u16,p:&[u8],_:u16)->Result<bool,SafetyError>{let begin=usize::from(y).checked_mul(self.stride).ok_or(SafetyError::RowSlice)?;let end=begin.checked_add(p.len()).ok_or(SafetyError::RowSlice)?;if p.len()>self.stride{return Err(SafetyError::RowSlice)}self.data.get_mut(begin..end).ok_or(SafetyError::RowSlice)?.copy_from_slice(p);Ok(true)}}
fn status(r:Result<(),RenderError>)->u8 {match r{Ok(())=>0,Err(RenderError::Status(s))=>s as u8,Err(RenderError::Safety(_))=>250}}
/// Safety: descriptor and every span must be valid, aligned and mutually disjoint;
/// even encoded bytes must not alias writable ranges. Ownership stays with caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_jpeg_baseline(r:*mut Runtime)->u8 {
    let Some(r)=(unsafe{r.as_mut()})else{return 250};
    macro_rules! take {($field:ident)=>{match unsafe{r.$field.borrow()}{Some(s)=>s,None=>return 250}}}
    let mut bytes=Bytes{data:take!(encoded),pos:0};let mut sink=Sink{data:take!(output),stride:r.stride};
    let w=Workspace{input:take!(input),segment:take!(segment),quant:take!(quant),huffman:take!(huffman),block:take!(block),mcu:take!(mcu),pixels:take!(pixels)};
    let rows=Rows{band:take!(band),row:take!(row),sums:take!(sums),counts:take!(counts)};
    status(renderer::render(&mut bytes,w,rows,r.width,r.height,&mut sink))
}
/// Same caller ownership contract as newt_jpeg_baseline. Four small Huffman tables
/// live on this wrapper's stack; grid and all rows are borrowed, never allocated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newt_jpeg_progressive(r:*mut Runtime)->u8 {
    let Some(r)=(unsafe{r.as_mut()})else{return 250};
    macro_rules! take {($field:ident)=>{match unsafe{r.$field.borrow()}{Some(s)=>s,None=>return 250}}}
    let mut bytes=Bytes{data:take!(encoded),pos:0};let mut sink=Sink{data:take!(output),stride:r.stride};let mut tables=[Huffman::default();4];
    let b=progressive::Buffers{input:take!(input),tables:&mut tables,grid:take!(band),previous:take!(segment),current:take!(row),output:take!(pixels)};
    status(progressive::render(&mut bytes,b,r.width,r.height,&mut sink))
}
// Non-generic Rust seams retain caller-borrowed paths in the real target crate.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn jpeg_gray_memory_runtime(bytes:&[u8],work:Workspace<'_>,rows:Rows<'_>,dst:(u16,u16),output:&mut[u8],stride:usize)->Result<(),RenderError>{
    newt_core::jpeg::runtime_probe::jpeg_gray_memory_runtime(bytes,work,rows,dst,output,stride)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn jpeg_progressive_memory_runtime(bytes:&[u8],work:progressive::Buffers<'_>,dst:(u16,u16),output:&mut[u8],stride:usize)->Result<(),RenderError>{
    newt_core::jpeg::runtime_probe::jpeg_progressive_memory_runtime(bytes,work,dst,output,stride)
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn jpeg_info_runtime(bytes:&[u8])->Option<newt_core::jpeg::detection::Info>{
    newt_core::jpeg::runtime_probe::jpeg_info_runtime(bytes)
}
