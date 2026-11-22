use newt_core::jpeg::{Decoder,Error,Input,Output,Rect,Workspace,renderer};
use newt_core::page_renderer::{ImageRows,SafetyError};
use std::io::{self,Read};
struct Request {mode:char,a:u16,b:u16,stop:usize,chunk:usize,fail:usize,data:Vec<u8>}
fn parse(raw:&[u8])->Option<Request>{
    if raw.len()>2100000||raw.last()!=Some(&b'\n')||raw[..raw.len()-1].iter().any(|b|*b!=b' '&&!(33..=126).contains(b)){return None}
    let s=std::str::from_utf8(&raw[..raw.len()-1]).ok()?;let t:Vec<_>=s.split(' ').collect();
    if t.len()!=8||t[0]!="JP1"||!matches!(t[1],"D"|"R"|"P"|"A"|"I"){return None}
    let num=|i:usize,max:usize|->Option<usize>{let s=t[i];let n=s.parse::<usize>().ok()?;if n>max||n.to_string()!=s{None}else{Some(n)}};
    let a=num(2,65535)?;let b=num(3,65535)?;let stop=num(4,65535)?;let chunk=num(5,1048576)?;let fail=num(6,1048576)?;
    let mode=t[1].chars().next()?;if chunk==0||(mode=='D'&&(a>3||b!=0))||(mode!='D'&&(a==0||b==0)){return None}
    let h=t[7].as_bytes();if h.is_empty()||h.len()%2!=0||h.len()>2097152||h.iter().any(|b|!b.is_ascii_digit()&&!(b'a'..=b'f').contains(b)){return None}
    let data=h.chunks_exact(2).map(|s|u8::from_str_radix(std::str::from_utf8(s).unwrap(),16).unwrap()).collect();
    Some(Request{mode,a:a as u16,b:b as u16,stop,chunk,fail,data})
}
struct Source<'a>{r:&'a Request,pos:usize,io:bool,coalesce:bool}
impl Input for Source<'_>{fn read(&mut self,dst:&mut[u8])->Result<usize,Error>{
    // Match jpegInput's whole-request short-read coalescing.
    let mut total=0;while total<dst.len(){if self.r.mode!='D' && self.pos==self.r.data.len(){break}if self.pos>=self.r.fail{self.io=true;return Err(Error::Input)}let n=(dst.len()-total).min(self.r.chunk).min(self.r.data.len()-self.pos);if n==0{break}dst[total..total+n].copy_from_slice(&self.r.data[self.pos..self.pos+n]);self.pos+=n;total+=n;if !self.coalesce{break}}Ok(total)
}}
struct Sink{events:Vec<String>,stop:usize}
fn hex(p:&[u8])->String {p.iter().map(|b|format!("{b:02x}")).collect()}
impl Output for Sink{fn write(&mut self,r:Rect,p:&[u8])->bool{self.events.push(format!("B:{}:{}:{}:{}:{}",r.left,r.top,r.right,r.bottom,hex(p)));self.stop==0||self.events.len()<self.stop}}
impl ImageRows for Sink{fn on_row(&mut self,y:u16,p:&[u8],_:u16)->Result<bool,SafetyError>{self.events.push(format!("O:{y}:{}",hex(p)));Ok(self.stop==0||self.events.len()<self.stop)}}
fn main(){let mut raw=Vec::new();if io::stdin().take(2100001).read_to_end(&mut raw).is_err(){std::process::exit(2)}let Some(r)=parse(&raw)else{std::process::exit(2)};
    let mut source=Source{r:&r,pos:0,io:false,coalesce:r.mode!='P'&&r.mode!='A'};let mut sink=Sink{events:Vec::new(),stop:r.stop};
    // Host allocations are explicit transport storage, not a target capacity claim.
    let mut input=vec![0;512];let mut segment=vec![0;512];let mut quant=vec![0;256];let mut huffman=vec![0;1088];let mut block=vec![0;64];let mut mcu=vec![0;384];let mut pixels=vec![0;256];

    let progressive=if r.mode=='A'{let info=newt_core::jpeg::detection::jpeg(&mut source);source.pos=0;source.io=false;info.map(|i|i.progressive)}else{Some(r.mode=='P')};
    source.coalesce=progressive==Some(false);
    let status=if r.mode=='I'{source.coalesce=false;if let Some(i)=newt_core::jpeg::detection::jpeg(&mut source){sink.events.push(format!("I:{}:{}:{}",i.width,i.height,u8::from(i.progressive)));}0}else if progressive.is_none(){0}else if progressive==Some(true){
      let mut tables=[newt_core::jpeg::progressive::Huffman::default();4];let mut grid=vec![0;8192*15];let mut previous=vec![0;usize::from(r.a)];let mut current=vec![0;usize::from(r.a)];let mut output=vec![0;usize::from(r.a)];
      match newt_core::jpeg::progressive::render(&mut source,newt_core::jpeg::progressive::Buffers{input:&mut input,tables:&mut tables,grid:&mut grid,previous:&mut previous,current:&mut current,output:&mut output},r.a,r.b,&mut sink){Ok(())=>0,Err(renderer::RenderError::Status(s))=>s as u8,Err(renderer::RenderError::Safety(_))=>250}
    }else if r.mode=='D'{match Decoder::prepare(Workspace{input:&mut input,segment:&mut segment,quant:&mut quant,huffman:&mut huffman,block:&mut block,mcu:&mut mcu,pixels:&mut pixels},&mut source){Err(e)=>e as u8,Ok(mut d)=>match d.decode(&mut source,&mut sink,r.a as u8){Ok(())=>0,Err(e)=>e as u8}}}
    else{let mut band=vec![0;65535*16];let mut row=vec![0;usize::from(r.a)];let mut sums=vec![0;usize::from(r.a)];let mut counts=vec![0;usize::from(r.a)];
      let result=renderer::render(&mut source,Workspace{input:&mut input,segment:&mut segment,quant:&mut quant,huffman:&mut huffman,block:&mut block,mcu:&mut mcu,pixels:&mut pixels},renderer::Rows{band:&mut band,row:&mut row,sums:&mut sums,counts:&mut counts},r.a,r.b,&mut sink);
      if source.io{newt_core::page_renderer::BookStatus::IoError as u8}else{match result{Ok(())=>0,Err(renderer::RenderError::Status(s))=>s as u8,Err(renderer::RenderError::Safety(_))=>250}}
    };
    print!("JP1 {} {}",status,sink.events.len());for e in sink.events{print!(" {e}")}println!();
}
