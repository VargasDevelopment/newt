//! Safe translation of frozen ImageRenderer's first-scan DC preview, not full JPEG decode.
use super::Input;
use crate::page_renderer::{BookStatus, ImageRows};
use super::renderer::RenderError;
type Result<T> = core::result::Result<T, BookStatus>;
#[derive(Clone, Copy)]
pub struct Huffman { counts:[u8;17], first:[u16;17], index:[u16;17], symbols:[u8;256], ready:bool }
impl Default for Huffman { fn default()->Self { Self{counts:[0;17],first:[0;17],index:[0;17],symbols:[0;256],ready:false} } }
pub struct Buffers<'a> { pub input:&'a mut[u8], pub tables:&'a mut[Huffman], pub grid:&'a mut[u8], pub previous:&'a mut[u8], pub current:&'a mut[u8], pub output:&'a mut[u8] }
struct Stream<'a,'b> { input:&'a mut dyn Input, buf:&'b mut[u8], pos:usize, len:usize, bits:u8, value:u8, marker:u8, io:bool }
impl Stream<'_,'_> {
    fn byte(&mut self)->Result<u8>{
        if self.pos==self.len { self.len=match self.input.read(&mut self.buf[..512]) { Ok(n) if n<=512=>n, _=>{self.io=true;return Err(BookStatus::ParseError)} };self.pos=0;if self.len==0{return Err(BookStatus::ParseError)} }
        let b=self.buf[self.pos];self.pos+=1;Ok(b)
    }
    fn word(&mut self)->Result<u16>{Ok(u16::from_be_bytes([self.byte()?,self.byte()?]))}
    fn skip(&mut self,n:usize)->Result<()> {for _ in 0..n {self.byte()?;}Ok(())}
    fn bit(&mut self)->Result<u8>{if self.bits==0 {let b=self.byte()?;if b==255 {let mut m=self.byte()?;while m==255{m=self.byte()?;}if m!=0 {self.marker=m;return Err(BookStatus::ParseError)}}self.value=b;self.bits=8;}self.bits-=1;Ok((self.value>>self.bits)&1)}
    fn symbol(&mut self,t:&Huffman)->Result<u8>{let mut code=0u32;for len in 1..=16 {code=code*2+u32::from(self.bit()?);let first=u32::from(t.first[len]);if t.counts[len]!=0&&code<first+u32::from(t.counts[len]) {let delta=code.checked_sub(first).ok_or(BookStatus::ParseError)?;return t.symbols.get(usize::from(t.index[len])+delta as usize).copied().ok_or(BookStatus::ParseError)}}Err(BookStatus::ParseError)}
    fn difference(&mut self,s:u8)->Result<i32>{let mut v=0;for _ in 0..s {v=v*2+i32::from(self.bit()?);}if s>0&&v<(1<<(s-1)){v-=(1<<s)-1;}Ok(v)}
    fn entropy_error(&self)->BookStatus {if self.io{BookStatus::IoError}else{BookStatus::ParseError}}
}
#[derive(Clone, Copy)]
struct Comp { id:u8,h:usize,v:usize,tq:usize,td:usize }
impl Default for Comp {fn default()->Self {Self{id:0,h:1,v:1,tq:0,td:0}}}
fn step(src:usize,dst:usize)->u32 {if src<=1||dst<=1{0}else{(((src-1)as u32)<<16)/(dst-1)as u32}}
fn position(i:usize,src:usize,dst:usize)->u32 {if i+1==dst{((src-1)as u32)<<16}else{(i as u32)*step(src,dst)}}
fn blend(a:u8,b:u8,f:u32)->u8 {((u32::from(a)*(65536-f)+u32::from(b)*f)>>16)as u8}
struct Scale<'a> { sw:usize,sh:usize,dw:usize,dh:usize,previous:&'a mut[u8],current:&'a mut[u8],output:&'a mut[u8],next:usize,seen:bool }
impl Scale<'_> {
    fn feed(&mut self,row:&[u8],y:usize,sink:&mut dyn ImageRows)->core::result::Result<bool,RenderError>{
        for x in 0..self.dw {let fp=position(x,self.sw,self.dw);let x0=(fp>>16)as usize;self.current[x]=blend(row[x0],row[(x0+1).min(self.sw-1)],fp&65535);}
        while self.next<self.dh {let fp=position(self.next,self.sh,self.dh);let y0=(fp>>16)as usize;let y1=(y0+1).min(self.sh-1);if y1>y{break}
            for x in 0..self.dw {self.output[x]=blend(if y0==y{self.current[x]}else{self.previous[x]},if y1==y{self.current[x]}else{self.previous[x]},fp&65535);}
            if !sink.on_row(self.next as u16,&self.output[..self.dw],self.dw as u16).map_err(RenderError::Safety)?{return Ok(false)}self.next+=1;
        }
        core::mem::swap(&mut self.previous,&mut self.current);self.seen=true;Ok(true)
    }
    fn finish(&mut self,sink:&mut dyn ImageRows)->core::result::Result<(),RenderError>{while self.next<self.dh&&self.seen {if !sink.on_row(self.next as u16,&self.previous[..self.dw],self.dw as u16).map_err(RenderError::Safety)?{break}self.next+=1;}Ok(())}
}
/// Buffers are borrowed and validated before use. Input preserves individual short reads.
pub fn render(input:&mut dyn Input,b:Buffers<'_>,width:u16,height:u16,sink:&mut dyn ImageRows)->core::result::Result<(),RenderError>{
    render_inner(input,b,width,height,sink)
}
fn render_inner(input:&mut dyn Input,b:Buffers<'_>,width:u16,height:u16,sink:&mut dyn ImageRows)->core::result::Result<(),RenderError>{
    use BookStatus::{ParseError as Parse,Unsupported,OutOfMemory};
    let err=RenderError::Status;
    if b.input.len()<512||b.tables.len()<4{return Err(err(OutOfMemory))}
    let mut s=Stream{input,buf:b.input,pos:0,len:0,bits:0,value:0,marker:0,io:false};
    if s.word().map_err(err)?!=0xffd8{return Err(err(Parse))}
    b.tables[..4].fill(Huffman::default());
    let(mut w,mut h,mut restart)=(0usize,0usize,0usize);let mut quant=[1u16;4];let mut comps=[Comp::default();4];let mut nc=0usize;let mut scan=[0usize;4];let ns;let al;
    loop {
        let mut m=s.byte().map_err(|_|err(s.entropy_error()))?;while m==255{m=s.byte().map_err(|_|err(s.entropy_error()))?;}
        if m==0xd8||(0xd0..=0xd7).contains(&m)||m==1{continue}
        let len=usize::from(s.word().map_err(err)?);if len<2{return Err(err(Parse))}let mut left=len-2;
        match m {
            0xdb=>while left>0 {let t=s.byte().map_err(err)?;let bytes=if t>>4!=0{128}else{64};let id=usize::from(t&15);if left<1+bytes||id>3{return Err(err(Parse))}quant[id]=if bytes==128{s.word().map_err(err)?}else{u16::from(s.byte().map_err(err)?)};s.skip(bytes-if bytes==128{2}else{1}).map_err(err)?;left-=1+bytes;},
            0xc4=>while left>0 {let t=s.byte().map_err(err)?;if left<17{return Err(err(Parse))}let mut counts=[0u8;17];let mut total=0;for c in &mut counts[1..] {*c=s.byte().map_err(err)?;total+=usize::from(*c);}if total>256||left<17+total{return Err(err(Parse))}
                if t>>4==0&&(t&15)<4 {let table=&mut b.tables[usize::from(t&15)];table.counts=counts;for sym in &mut table.symbols[..total]{*sym=s.byte().map_err(err)?;}let(mut code,mut index)=(0u16,0u16);for i in 1..=16 {table.first[i]=code;table.index[i]=index;code=code.wrapping_add(u16::from(counts[i])).wrapping_shl(1);index+=u16::from(counts[i]);}table.ready=true;}else{s.skip(total).map_err(err)?;}left-=17+total;
            },
            0xc2=>{s.byte().map_err(err)?;h=usize::from(s.word().map_err(err)?);w=usize::from(s.word().map_err(err)?);nc=usize::from(s.byte().map_err(err)?);if nc==0||nc>4||left!=6+nc*3{return Err(err(Parse))}for c in &mut comps[..nc] {c.id=s.byte().map_err(err)?;let hv=s.byte().map_err(err)?;c.h=usize::from(hv>>4);c.v=usize::from(hv&15);c.tq=usize::from(s.byte().map_err(err)?&3);if c.h==0||c.v==0{return Err(err(Parse))}}},
            0xc0|0xc1|0xd9=>return Err(err(Parse)),
            0xdd=>{if left!=2{return Err(err(Parse))}restart=usize::from(s.word().map_err(err)?);},
            0xda=>{ns=usize::from(s.byte().map_err(err)?);if ns==0||ns>4||left!=1+ns*2+3{return Err(err(Parse))}for slot in &mut scan[..ns] {let id=s.byte().map_err(err)?;let td=s.byte().map_err(err)?>>4;*slot=comps[..nc].iter().rposition(|c|c.id==id).ok_or(err(Parse))?;comps[*slot].td=usize::from(td);}let ss=s.byte().map_err(err)?;let se=s.byte().map_err(err)?;let aa=s.byte().map_err(err)?;if ss!=0||se!=0||aa>>4!=0{return Err(err(Unsupported))}al=aa&15;break;},
            _=>s.skip(left).map_err(err)?,
        }
    }
    if w==0||h==0{return Err(err(Parse))}
    let hm=comps[..nc].iter().map(|c|c.h).max().unwrap_or(1);let vm=comps[..nc].iter().map(|c|c.v).max().unwrap_or(1);let y=comps[0];let inter=ns>1;
    // Only Y's DC draws greys, so a first scan without Y has nothing to show.
    if y.h!=hm||y.v!=vm||!scan[..ns].contains(&0){return Err(err(Unsupported))}
    let bw=(w+7)/8;let bh=(h+7)/8;let mx=(w+8*hm-1)/(8*hm);let my=(h+8*vm-1)/(8*vm);let pad=if inter{mx*y.h}else{bw};let dw=usize::from(width);let dh=usize::from(height);
    if dw==0||dh==0{return Err(err(Parse))}if b.grid.len()<pad*y.v||b.previous.len()<dw||b.current.len()<dw||b.output.len()<dw{return Err(err(OutOfMemory))}
    b.grid[..pad*y.v].fill(0);
    let mut rs=Scale{sw:bw,sh:bh,dw,dh,previous:b.previous,current:b.current,output:b.output,next:0,seen:false};let mut pred=[0i32;4];let mut since=0;let mut emitted=0;
    for _r in 0..if inter{my}else{bh} {for c in 0..if inter{mx}else{bw} {
        if restart!=0&&since==restart {s.bits=0;let mut mk=s.marker;s.marker=0;if mk==0{mk=s.byte().map_err(err)?;while mk==255{mk=s.byte().map_err(err)?;}}if !(0xd0..=0xd7).contains(&mk){return Err(err(Parse))}pred=[0;4];since=0;}
        for &ci in &scan[..if inter{ns}else{1}] {let comp=comps[ci];let table=b.tables.get(comp.td).filter(|t|t.ready).ok_or(err(Parse))?;for by in 0..if inter{comp.v}else{1} {for bx in 0..if inter{comp.h}else{1} {
            let t=s.symbol(table).map_err(|_|err(s.entropy_error()))?;if t>15{return Err(err(s.entropy_error()))}let diff=s.difference(t).map_err(|_|err(s.entropy_error()))?;pred[ci]=pred[ci].checked_add(diff).ok_or(err(Parse))?;
            if ci==0 {let dc=pred[ci].checked_mul(1<<al).and_then(|v|v.checked_mul(i32::from(quant[y.tq]))).ok_or(err(Parse))?;let gray=(128+dc/8).clamp(0,255)as u8;let px=if inter{c*y.h+bx}else{c};if px<pad{b.grid[by*pad+px]=gray;}}
        }}}
        since+=1;
    }for by in 0..if inter{y.v}else{1} {if emitted>=bh{break}if !rs.feed(&b.grid[by*pad..by*pad+bw],emitted,sink)?{return Ok(())}emitted+=1;}}
    rs.finish(sink)
}
