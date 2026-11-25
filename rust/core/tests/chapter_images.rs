//! Source reads are the witness: Unknown is an authoritative cached result.
use newt_core::{chapter_layout::{images::{BookImages,Catalog,Probed},types::Images},member_check::Checked,zip_container::Status,zip_window::source::BookSource};
struct Source { calls:Vec<(u64,usize)> }
impl BookSource for Source {
    fn size(&self)->u64 {26}
    fn read_at(&mut self,at:u64,out:&mut [u8])->i32 {
        self.calls.push((at,out.len()));
        let n=out.len().min(26usize.saturating_sub(at as usize));
        out[..n].fill(0); n as i32
    }
}
struct Index;
impl Catalog for Index {
    fn member(&self,_:&mut dyn BookSource,p:&[u8])->Result<Checked,Status>{(p==b"img.png").then_some(Checked::raw(26)).ok_or(Status::NotFound)}
    fn member_by_hash(&self,_:&mut dyn BookSource,h:u32)->Result<Checked,Status>{(h==newt_core::css::hash(b"img.png")).then_some(Checked::raw(26)).ok_or(Status::NotFound)}
}
#[test]
fn cached_unknown_has_no_reads_inline_unknown_reads_once_per_probe() {
    let mut source=Source{calls:Vec::new()};
    let mut skip=[0;1024];let mut paths=[0;128];
    let cache=[Probed{hash:newt_core::css::hash(b"img.png"),..Probed::default()}];
    {
        let mut images=BookImages{source:&mut source,catalog:Some(&Index),directory:b"",decoder:None,skip:&mut skip,path_scratch:&mut paths,table:&cache};
        assert_eq!(images.probe(b"img.png"),None);
        assert_eq!(images.probe(b"img.png"),None);
    }
    assert_eq!(source.calls,vec![]);
    {
        let mut images=BookImages{source:&mut source,catalog:Some(&Index),directory:b"",decoder:None,skip:&mut skip,path_scratch:&mut paths,table:&[]};
        assert_eq!(images.probe(b"img.png"),None);
        assert_eq!(images.probe(b"img.png"),None);
    }
    assert_eq!(source.calls,vec![(0,26),(0,26)]);
}
