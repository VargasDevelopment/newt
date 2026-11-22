//! JPEG branch of frozen ImageProbe. Catalog dispatch and PNG detection remain external.
use super::Input;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Info {pub width:u16,pub height:u16,pub progressive:bool}
fn exact(input:&mut dyn Input,dst:&mut[u8])->Option<()> {let mut got=0;while got<dst.len(){let n=input.read(&mut dst[got..]).ok()?;if n==0||n>dst.len()-got{return None}got+=n;}Some(())}
struct Probe<'a> {input:&'a mut dyn Input,head:[u8;26],pending:usize,consumed:usize}
impl Probe<'_> {
    fn next(&mut self,dst:&mut[u8])->Option<()> {let n=dst.len().min(26-self.pending);dst[..n].copy_from_slice(&self.head[self.pending..self.pending+n]);self.pending+=n;if n<dst.len(){exact(self.input,&mut dst[n..])?;self.consumed+=dst.len()-n;}Some(())}
    fn skip(&mut self,n:usize)->Option<()> {let take=n.min(26-self.pending);self.pending+=take;let mut left=n-take;let mut buf=[0;1024];while left>0 {let want=left.min(buf.len());let got=self.input.read(&mut buf[..want]).ok()?;if got==0||got>want{return None}left-=got;self.consumed+=got;}Some(())}
}
/// Like the native probe, short/failed headers yield an unrecognized image.
/// The caller must reopen its entry before decoding; this is not a catalog dispatcher.
pub fn jpeg(input:&mut dyn Input)->Option<Info> {
    let mut head=[0;26];exact(input,&mut head)?;if head[..2]!=[255,216]{return None}
    let mut p=Probe{input,head,pending:2,consumed:26};let mut seg=[0;2];
    while p.consumed<128*1024 {p.next(&mut seg)?;if seg[0]!=255{return None}let m=seg[1];if m==0xd8||(0xd0..=0xd7).contains(&m)||m==1{continue}if m==0xd9||m==0xda{return None}p.next(&mut seg)?;let len=usize::from(u16::from_be_bytes(seg));if len<2{return None}
        if (0xc0..=0xcf).contains(&m)&&!matches!(m,0xc4|0xc8|0xcc){let mut sof=[0;5];p.next(&mut sof)?;return Some(Info{width:u16::from_be_bytes([sof[3],sof[4]]),height:u16::from_be_bytes([sof[1],sof[2]]),progressive:m==0xc2})}p.skip(len-2)?;
    }None
}
