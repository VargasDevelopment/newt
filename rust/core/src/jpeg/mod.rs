#![forbid(unsafe_code)]
mod tables;
pub mod renderer;
pub mod progressive;
pub mod detection;
pub mod runtime_probe;
use self::tables::{IPS,ZIG};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(u8)]
pub enum Error { Interrupted=1, Input=2, Memory=3, Segment=4, Parameter=5, Format=6, Unsupported=8, Arithmetic=9 }
type Result<T> = core::result::Result<T,Error>;
pub trait Input {
    /// Return bytes read; zero is EOF. An error aborts the decode.
    fn read(&mut self, dst: &mut [u8]) -> Result<usize>;
}
#[derive(Clone,Copy,Debug)]
pub struct Rect { pub left:u16,pub top:u16,pub right:u16,pub bottom:u16 }
pub trait Output { fn write(&mut self,rect:Rect,pixels:&[u8])->bool; }
/// All large storage is supplied by the caller. No allocator or hidden full-image buffer.
pub struct Workspace<'a> {
    pub input: &'a mut [u8], // >=512
    pub segment: &'a mut [u8], // >=512
    pub quant: &'a mut [i32], // >=256
    pub huffman: &'a mut [u8], // >=1088: four (16 counts +256 symbols)
    pub block: &'a mut [i32], // >=64
    pub mcu: &'a mut [i16], // >=384
    pub pixels: &'a mut [u8], // >=256 (pinned grayscale configuration)
}
pub struct Decoder<'a> {
    w:Workspace<'a>, pub width:u16,pub height:u16,
    mx:usize,my:usize,ncomp:usize,qt:[usize;3],qset:[bool;4],hset:[bool;4],
    restart:u16,dc:[i16;3],pos:usize,end:usize,offset:usize,reg:u32,bits:u8,marker:u8,
}
fn exact(input:&mut dyn Input,dst:&mut[u8])->Result<()> {
    // ImageRenderer's jpegInput coalesces short reads and treats errors as fatal.
    let mut n=0;
    while n<dst.len() {let k=input.read(&mut dst[n..])?;if k==0||k>dst.len()-n{return Err(Error::Input)} n+=k;}
    Ok(())
}
fn word(s:&[u8],i:usize)->u16 {u16::from_be_bytes([s[i],s[i+1]])}
impl<'a> Decoder<'a> {
    pub fn prepare(w:Workspace<'a>,input:&mut dyn Input)->Result<Self> {
        if w.input.len()<512||w.segment.len()<512||w.quant.len()<256||w.huffman.len()<1088||w.block.len()<64||w.mcu.len()<384||w.pixels.len()<256{return Err(Error::Memory)}
        let mut d=Self{w,width:0,height:0,mx:0,my:0,ncomp:0,qt:[0;3],qset:[false;4],hset:[false;4],restart:0,dc:[0;3],pos:0,end:0,offset:0,reg:0,bits:0,marker:0};
        let mut mark=0u16;
        while mark!=0xffd8 {exact(input,&mut d.w.segment[..1])?;d.offset=d.offset.wrapping_add(1);mark=(mark<<8)|u16::from(d.w.segment[0]);}
        loop {
            exact(input,&mut d.w.segment[..4])?;
            let m=word(d.w.segment,0);let len=usize::from(word(d.w.segment,2));
            if m>>8!=255||len<=2{return Err(Error::Format)}
            let len=len-2;d.offset=d.offset.wrapping_add(len+4);
            if !matches!(m,0xffc0|0xffc4|0xffdb|0xffdd|0xffda) {
                if matches!(m&255,0xc1..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf|0xd9){return Err(Error::Unsupported)}
                let mut left=len;while left>0 {let n=left.min(512);exact(input,&mut d.w.segment[..n])?;left-=n;}continue
            }
            if len>512{return Err(Error::Segment)}
            exact(input,&mut d.w.segment[..len])?;
            let s=&d.w.segment[..len];
            match m&255 {
                0xc0=>{
                    if len<6{return Err(Error::Format)}
                    d.height=word(s,1);d.width=word(s,3);d.ncomp=usize::from(s[5]);
                    if s[0]!=8||!matches!(d.ncomp,1|3){return Err(Error::Unsupported)}
                    if len!=6+3*d.ncomp{return Err(Error::Format)}
                    // Past native's 11/21/22, Y 12 and 41 still fit six MCU blocks and 256 pixels.
                    for i in 0..d.ncomp {
                        let b=s[7+3*i];
                        if i==0 {if !matches!(b,0x11|0x12|0x21|0x22|0x41){return Err(Error::Unsupported)}d.mx=usize::from(b>>4);d.my=usize::from(b&15);}
                        else if b!=0x11{return Err(Error::Unsupported)}
                        d.qt[i]=usize::from(s[8+3*i]);if d.qt[i]>3{return Err(Error::Unsupported)}
                    }
                }
                0xdd=>{if len!=2{return Err(Error::Format)}d.restart=word(s,0);}
                0xdb=>{
                    let mut p=0;while p<len {if len-p<65||s[p]&0xf0!=0{return Err(Error::Format)}
                        let id=usize::from(s[p]&3);d.qset[id]=true;
                        for z in 0..64 {let i=ZIG[z];d.w.quant[id*64+i]=i32::from(s[p+1+z])*IPS[i];}p+=65;
                    }
                }
                0xc4=>{
                    let mut p=0;while p<len {
                        if len-p<17||s[p]&0xee!=0{return Err(Error::Format)}
                        let id=usize::from((s[p]&1)*2+(s[p]>>4));let base=id*272;
                        // TJpgDec accepts over-subscribed counts. Its uint16 code words wrap only after
                        // every bit pattern already matches a shorter code, so huff() needs no wrap.
                        let mut count=0usize;
                        for b in 0..16 {let n=s[p+1+b];count+=usize::from(n);d.w.huffman[base+b]=n;}
                        if count>256||len-p-17<count{return Err(Error::Format)}
                        for i in 0..count {let v=s[p+17+i];if id%2==0&&v>11{return Err(Error::Format)}d.w.huffman[base+16+i]=v;}
                        d.hset[id]=true;p+=17+count;
                    }
                }
                0xda=>{
                    if d.width==0||d.height==0{return Err(Error::Format)}
                    // Native reads only the count and each component's table byte.
                    if len<1+2*d.ncomp{return Err(Error::Format)}
                    if usize::from(s[0])!=d.ncomp{return Err(Error::Unsupported)}
                    for i in 0..d.ncomp {
                        if !matches!(s[2+2*i],0|0x11){return Err(Error::Unsupported)}
                        let id=if i==0{0}else{2};
                        if !d.hset[id]||!d.hset[id+1]||!d.qset[d.qt[i]]{return Err(Error::Format)}
                    }
                    // Native ignores the spectral/approximation bytes and any trailing bytes.
                    let start=d.offset%512;
                    if start!=0 {d.end=input.read(&mut d.w.input[start..512])?;if d.end>512-start{return Err(Error::Input)}d.pos=start;d.end+=start;}
                    return Ok(d)
                }
                _=>unreachable!(),
            }
        }
    }
    fn byte(&mut self,input:&mut dyn Input)->Result<u8> {
        if self.pos==self.end {self.end=input.read(&mut self.w.input[..512])?;self.pos=0;if self.end==0||self.end>512{return Err(Error::Input)}}
        let b=self.w.input[self.pos];self.pos+=1;Ok(b)
    }
    fn fill(&mut self,input:&mut dyn Input,n:u8)->Result<()> {
        self.reg &= if self.bits==0{0}else{(1u32<<self.bits)-1};
        while self.bits<n {
            let b=if self.marker!=0{255}else{let b=self.byte(input)?;if b==255 {let m=self.byte(input)?;if m!=0{self.marker=m;}255}else{b}};
            self.reg=(self.reg<<8)|u32::from(b);self.bits+=8;
        }Ok(())
    }
    fn huff(&mut self,input:&mut dyn Input,id:usize)->Result<u8> {
        self.fill(input,16)?;
        let mut code=0u32;let mut p=0;
        for b in 1..=16 {let n=usize::from(self.w.huffman[id*272+b-1]);let v=self.reg>>(self.bits-b as u8);
            if v>=code&&v<code+n as u32 {self.bits-=b as u8;return Ok(self.w.huffman[id*272+16+p+(v-code) as usize])}
            p+=n;code=(code+n as u32)<<1;
        }Err(Error::Format)
    }
    fn value(&mut self,input:&mut dyn Input,n:u8)->Result<i32> {
        if n==0{return Ok(0)}if n>16{return Err(Error::Format)}self.fill(input,n)?;self.bits-=n;
        let v=((self.reg>>self.bits)&((1<<n)-1)) as i32;
        Ok(if v<(1<<(n-1)){v-((1<<n)-1)}else{v})
    }
    fn load(&mut self,input:&mut dyn Input,scale:u8)->Result<()> {
        let ny=self.mx*self.my;
        for b in 0..ny+2 {
            let cmp=if b<ny{0}else{b-ny+1};
            if cmp!=0&&self.ncomp==1 {self.w.mcu[b*64..b*64+64].fill(128);continue}
            let id=if cmp==0{0}else{2};let n=self.huff(input,id)?;
            let dc=i32::from(self.dc[cmp])+self.value(input,n)?;
            // TJpgDec narrows only the stored predictor (two's complement); this block keeps the full value.
            self.dc[cmp]=dc as i16;
            self.w.block[..64].fill(0);
            self.w.block[0]=dequant(dc,self.w.quant[self.qt[cmp]*64])?;
            let mut z=1;
            while z<64 {let h=self.huff(input,id+1)?;if h==0{break}z+=usize::from(h>>4);if z>=64{return Err(Error::Format)}
                let n=h&15;if n!=0 {let v=self.value(input,n)?;let i=ZIG[z];self.w.block[i]=dequant(v,self.w.quant[self.qt[cmp]*64+i])?;}z+=1;
            }
            if cmp==0 {
                if z==1||scale==3 {let v=(self.w.block[0]/256+128) as i16;self.w.mcu[b*64..b*64+64].fill(v);}
                else {idct(self.w.block,&mut self.w.mcu[b*64..b*64+64])?;}
            }
        }Ok(())
    }
    /// Decode the scan once. Output false is JDR_INTR; no further MCU is loaded.
    pub fn decode(&mut self,input:&mut dyn Input,out:&mut dyn Output,scale:u8)->Result<()> {
        if scale>3{return Err(Error::Parameter)}
        self.dc=[0;3];let mut rst=0u32;let mut seq=0u8;
        let mx=self.mx*8;let my=self.my*8;
        for y in (0..usize::from(self.height)).step_by(my) {for x in (0..usize::from(self.width)).step_by(mx) {
            if self.restart!=0&&rst==u32::from(self.restart) {
                let marker=if self.marker!=0 {let m=self.marker;self.marker=0;0xff00|u16::from(m)}else{let a=self.byte(input)?;let b=self.byte(input)?;u16::from_be_bytes([a,b])};
                if marker&0xffd8!=0xffd0||marker&7!=u16::from(seq&7){return Err(Error::Format)}
                seq=seq.wrapping_add(1);self.bits=0;self.dc=[0;3];rst=0;
            }rst+=1;self.load(input,scale)?;
            let rx=(mx.min(usize::from(self.width)-x))>>scale;let ry=(my.min(usize::from(self.height)-y))>>scale;
            if rx==0||ry==0{continue}
            let w=1usize<<scale;
            for oy in 0..ry {for ox in 0..rx {
                let value=if scale==3 {let block=oy*self.mx+ox;self.w.mcu[block*64] as u8}
                else {let mut sum=0u32;for dy in 0..w {for dx in 0..w {let px=ox*w+dx;let py=oy*w+dy;let b=(py/8)*self.mx+px/8;sum+=u32::from(self.w.mcu[b*64+(py%8)*8+px%8] as u8);}}(sum>>(scale*2)) as u8};
                self.w.pixels[oy*rx+ox]=value;
            }}
            let left=(x>>scale) as u16;let top=(y>>scale) as u16;
            if !out.write(Rect{left,top,right:left+rx as u16-1,bottom:top+ry as u16-1},&self.w.pixels[..rx*ry]){return Err(Error::Interrupted)}
        }}Ok(())
    }
}
fn dequant(v:i32,q:i32)->Result<i32> {Ok(v.checked_mul(q).ok_or(Error::Arithmetic)?>>8)}
// Reject native signed-overflow domain rather than relying on build-mode wrap.
fn add(a:i32,b:i32)->Result<i32>{a.checked_add(b).ok_or(Error::Arithmetic)}
fn sub(a:i32,b:i32)->Result<i32>{a.checked_sub(b).ok_or(Error::Arithmetic)}
fn mul(a:i32,b:i32)->Result<i32>{a.checked_mul(b).ok_or(Error::Arithmetic)}
fn butterfly(s:[i32;8])->Result<[i32;8]> {
    let t10=add(s[0],s[4])?;let t12=sub(s[0],s[4])?;
    let t11=mul(sub(s[2],s[6])?,5792)?>>12;
    let v3=add(s[6],s[2])?;let t11=sub(t11,v3)?;
    let v0=add(t10,v3)?;let v3=sub(t10,v3)?;
    let v1=add(t11,t12)?;let v2=sub(t12,t11)?;
    let t10=sub(s[1],s[7])?;let t11=add(s[1],s[7])?;
    let t12=sub(s[5],s[3])?;let v7=add(s[3],s[5])?;
    let v5=mul(sub(t11,v7)?,5792)?>>12;
    let v7=add(v7,t11)?;
    let t13=mul(add(t10,t12)?,7568)?>>12;
    let v4=sub(t13,mul(t10,4433)?>>12)?;
    let v6=sub(sub(t13,mul(t12,10703)?>>12)?,v7)?;
    let v5=sub(v5,v6)?;let v4=sub(v4,v5)?;
    Ok([add(v0,v7)?,add(v1,v6)?,add(v2,v5)?,add(v3,v4)?,sub(v3,v4)?,sub(v2,v5)?,sub(v1,v6)?,sub(v0,v7)?])
}
fn idct(src:&mut[i32],dst:&mut[i16])->Result<()> {
    for x in 0..8 {let a=butterfly(core::array::from_fn(|y|src[y*8+x]))?;for y in 0..8{src[y*8+x]=a[y];}}
    for y in 0..8 {let mut a=core::array::from_fn(|x|src[y*8+x]);a[0]=add(a[0],128<<8)?;let a=butterfly(a)?;for x in 0..8{dst[y*8+x]=(a[x]>>8) as i16;}}
    Ok(())
}
