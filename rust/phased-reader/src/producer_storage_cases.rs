extern crate std;
use super::*;
use core::cell::Cell;
use newt_core::{producer_policy::{Hooks,Stop},chapter_layout::types::LineBreak,xml_session::{ParserBackend,Handler}};
#[repr(align(16))]
struct Native([u8;newt_phased_sink::FRAME_BYTES+15]);
struct Loan<'a>{native:&'a mut[u8],returns:u32}
impl BuildLoan for Loan<'_>{
    fn with_parse<R>(&mut self,f:impl for<'a>FnOnce(&'a mut[u8])->R)->Result<R,Status>{
        let result=f(self.native);self.native.fill(0xff);self.returns+=1;Ok(result)
    }
}
#[derive(Default)]
struct Cache{data:std::vec::Vec<u8>}
impl CacheStorage for Cache{
    fn remove(&mut self,_:&[u8])->bool{true}
    fn file_size(&mut self,_:&[u8])->i64{self.data.len() as i64}
    fn read_at(&mut self,_:&[u8],_:u32,_:&mut[u8])->i32{0}
    fn begin_write(&mut self,_:&[u8])->bool{true}
    fn write(&mut self,b:&[u8])->bool{self.data.extend_from_slice(b);true}
    fn end_write(&mut self)->bool{true}
}
struct Source;
impl BookSource for Source{fn size(&self)->u64{0}fn read_at(&mut self,_:u64,_:&mut[u8])->i32{0}}
fn stop(_: &())->Stop{Stop::None}
fn attempt()->Cancellation<'static,()>{Cancellation::new(Hooks{context:&(),stop:Some(stop),yield_now:None,pause:None})}
#[test]
fn all_sixteen_native_alignments_keep_every_producer_object_in_tail(){
    for offset in 0..16{
        let mut native=Native([0xa5;newt_phased_sink::FRAME_BYTES+15]);let base=native.0[offset..].as_ptr() as usize;
        let mut loan=Loan{native:&mut native.0[offset..offset+newt_phased_sink::FRAME_BYTES],returns:0};
        let mut shared=[0;BANK_BYTES];let mut storage=Storage::new(&mut shared);let c=attempt();let mut cache=Cache::default();let mut source=Source;
        storage.with_producer(&mut loan,&c,&mut cache as &mut dyn CacheStorage,&mut source,300,None,|owner| owner.with_buffered(|phase,mut attempt|{attempt.with_writer(|a|{
            assert_eq!(phase.parse.len(),BANK_BYTES-BUFFER_BYTES);
            let aligned=(base+15)&!15;let begin=aligned+BANK_BYTES;let end=base+newt_phased_sink::FRAME_BYTES;
            let mut ranges=std::vec![(a.state as *mut _ as usize,core::mem::size_of_val(a.state)),
                (a.writer as *mut _ as usize,core::mem::size_of_val(a.writer)),(a.store as *mut _ as usize,core::mem::size_of_val(a.store)),
                (a.source as *mut _ as usize,core::mem::size_of_val(a.source)),(a.writer.starts.as_ptr() as usize,128),
                (a.memory.chunks.as_ptr() as usize,1024),(a.memory.anchors.as_ptr() as usize,1536)];
            ranges.sort_unstable();for &(p,n) in &ranges{assert!(p>=begin && p+n<=end);}
            for pair in ranges.windows(2){assert!(pair[0].0+pair[0].1<=pair[1].0);}
            assert_eq!(a.state.count,3);assert_eq!(a.memory.chunks.len(),1);assert_eq!(a.memory.anchors.len(),192);
            Ok(())
        })?})).unwrap();assert_eq!(loan.returns,1);assert!(loan.native.iter().all(|&b|b==0xff));
    }
}
struct NoParser;
impl ParserBackend for NoParser{fn create(&mut self)->bool{false}fn feed(&mut self,_:&[u8],_:bool,_:&mut dyn Handler)->bool{false}fn close(&mut self){}}
struct NoBreak;
impl LineBreak for NoBreak{fn breaks(&mut self,_:&[u8],_:Option<&core::ffi::CStr>,_:&mut[u8]){panic!("resource construction must not line-break")}}
#[test]
fn normal_layout_io_still_fits_without_touching_the_lent_stream_window(){
    let mut native=Native([0xa5;newt_phased_sink::FRAME_BYTES+15]);let mut loan=Loan{native:&mut native.0[..newt_phased_sink::FRAME_BYTES],returns:0};
    let mut shared=[0;BANK_BYTES];let mut storage=Storage::new(&mut shared);let c=attempt();let mut cache=Cache::default();let mut source=Source;
    storage.with_producer(&mut loan,&c,&mut cache,&mut source,300,None,|owner| owner.with_buffered(|phase,mut attempt|{attempt.with_writer(|a|{
        a.begin_stream();
        assert!(a.store.write(&[0x5a;1000]));phase.defer(0)?;
        let mut parser=NoParser;let mut breaker=NoBreak;
        phase.layout(&mut parser,&mut breaker,|slot,lr,io|{
            assert_eq!(slot.spine,0);assert_eq!(lr.storage.text.len(),3072);
            io.xml_buffer.as_deref_mut().unwrap().fill(0x32);
            let decoder=io.decoder.as_mut().unwrap();decoder.dictionary.fill(0x44);decoder.input.fill(0x55);
        })?;
        assert!(a.store.end_write());Ok(())
    })?})).unwrap();assert_eq!(cache.data,std::vec![0x5a;1000]);assert_eq!(loan.returns,1);assert!(storage.styles_ready(0));
}
#[test]
fn short_native_storage_refuses_before_callback_and_preserves_shared_bytes(){
    let mut native=Native([0xa5;newt_phased_sink::FRAME_BYTES+15]);let mut loan=Loan{native:&mut native.0[..newt_phased_sink::FRAME_BYTES-1],returns:0};
    let mut shared=[0x6a;BANK_BYTES];let mut storage=Storage::new(&mut shared);let c=attempt();let mut cache=Cache::default();let mut source=Source;let calls=Cell::new(0);
    assert_eq!(storage.with_producer(&mut loan,&c,&mut cache,&mut source,300,None,|owner| owner.with_buffered(|_,_|{calls.set(calls.get()+1);Ok(())})),Err(Status::OutOfMemory));
    assert_eq!(calls.get(),0);assert_eq!(loan.returns,1);assert!(cache.data.is_empty());assert!(shared.iter().all(|&b|b==0x6a));
}

#[test]
fn full_bank_returns_after_writing_window() {
    let mut native = Native([0xa5; newt_phased_sink::FRAME_BYTES + 15]);
    let mut loan = Loan { native: &mut native.0[..newt_phased_sink::FRAME_BYTES], returns: 0 };
    let mut shared = [0; BANK_BYTES];
    let mut storage = Storage::new(&mut shared);
    let cancellation = attempt();
    let mut cache = Cache::default();
    let mut source = Source;
    let sizes = storage.with_producer(&mut loan, &cancellation, &mut cache, &mut source, 1,None, |owner| {
        let count = owner.with_unbuffered(|phase, _| Ok(phase.parse.len()))?;
        let writing = owner.with_buffered(|phase, _| Ok(phase.parse.len()))?;
        let settlement = owner.with_unbuffered(|phase, _| Ok(phase.parse.len()))?;
        Ok([count, writing, settlement])
    }).unwrap();
    assert_eq!(loan.returns, 1);
    assert!(loan.native.iter().all(|b| *b == 0xff));
    std::println!("U {} {} {}", sizes[0], sizes[1], sizes[2]);
}

// This SDK-fake source has no native receipt; the capability is explicit.
impl newt_core::producer_policy::ProducerReceipt for Source {
    fn producer_receipt_live(&mut self) -> bool { true }
}

#[test]
fn layout_with_capture_carves_shared_bank_remainder_and_restores_home_at_rest() {
    for offset in 0..16 {
        let mut native = Native([0xa5; newt_phased_sink::FRAME_BYTES + 15]);
        let mut loan = Loan {
            native: &mut native.0[offset..offset + newt_phased_sink::FRAME_BYTES],
            returns: 0,
        };
        let mut shared = [0x11; BANK_BYTES];
        let shared_lo = shared.as_ptr() as usize;
        let shared_hi = shared_lo + BANK_BYTES;
        let mut storage = Storage::new(&mut shared);
        let c = attempt();
        let mut cache = Cache::default();
        let mut source = Source;
        storage
            .with_producer(&mut loan, &c, &mut cache, &mut source, 300, None, |owner| {
                owner.with_buffered(|phase, mut attempt| {
                    attempt.with_writer(|a| {
                        a.begin_stream();
                        assert!(a.store.write(&[0x5a; 1000]));
                        phase.defer(0)?;
                        phase.edit_slot(|slot| {
                            slot.rules[0].elem_hash = 0x1234_5678;
                            slot.rules[0].decl.weight_bold = 1;
                            slot.count = 1;
                            slot.hash = 0xfeed_face;
                            slot.state = crate::styles::State::Settled;
                        })?;
                        let mut parser = NoParser;
                        let mut breaker = NoBreak;
                        phase.layout_with_capture(
                            &mut parser,
                            &mut breaker,
                            true,
                            |slot, lr, io, capture| {
                                assert_eq!(slot.spine, 0);
                                assert_eq!(slot.count, 1);
                                assert_eq!(slot.hash, 0xfeed_face);
                                let cap = capture.expect("capture loan must carve from shared tail");
                                let mut layout_ranges = std::vec![
                                    (lr.storage.text.as_ptr() as usize, lr.storage.text.len()),
                                    (lr.storage.breaks.as_ptr() as usize, lr.storage.breaks.len()),
                                    (lr.storage.levels.as_ptr() as usize, lr.storage.levels.len()),
                                    (
                                        lr.storage.spans.as_ptr() as usize,
                                        core::mem::size_of_val(lr.storage.spans),
                                    ),
                                    (
                                        lr.storage.lines.as_ptr() as usize,
                                        core::mem::size_of_val(lr.storage.lines),
                                    ),
                                    (
                                        lr.storage.elements.as_ptr() as usize,
                                        core::mem::size_of_val(lr.storage.elements),
                                    ),
                                    (
                                        lr.storage.runs.as_ptr() as usize,
                                        core::mem::size_of_val(lr.storage.runs),
                                    ),
                                    (
                                        lr.storage.images.as_ptr() as usize,
                                        core::mem::size_of_val(lr.storage.images),
                                    ),
                                    (
                                        lr.storage.links.as_ptr() as usize,
                                        core::mem::size_of_val(lr.storage.links),
                                    ),
                                    (lr.storage.page.as_ptr() as usize, lr.storage.page.len()),
                                    (
                                        lr.storage.style_text.as_ptr() as usize,
                                        lr.storage.style_text.len(),
                                    ),
                                    (
                                        lr.storage.paragraph_links.as_ptr() as usize,
                                        core::mem::size_of_val(lr.storage.paragraph_links),
                                    ),
                                    (
                                        lr.images.path_scratch.as_ptr() as usize,
                                        lr.images.path_scratch.len(),
                                    ),
                                ];
                                let cap_ranges = [
                                    (cap as *mut _ as usize, core::mem::size_of_val(cap)),
                                    (cap.live_bits.as_ptr() as usize, cap.live_bits.len()),
                                ];
                                for &(p, n) in &cap_ranges {
                                    assert!(p >= shared_lo && p + n <= shared_hi);
                                    for &(lp, ln) in &layout_ranges {
                                        assert!(p + n <= lp || lp + ln <= p);
                                    }
                                }
                                layout_ranges.extend_from_slice(&cap_ranges);
                                layout_ranges.sort_unstable();
                                for pair in layout_ranges.windows(2) {
                                    assert!(pair[0].0 + pair[0].1 <= pair[1].0);
                                }
                                cap.live_bits.fill(0xcd);
                                cap.tap.observe(&[0xcd; 512]);
                                io.xml_buffer.as_deref_mut().unwrap().fill(0x32);
                                let dec = io.decoder.as_mut().unwrap();
                                dec.dictionary.fill(0x44);
                                dec.input.fill(0x55);
                            },
                        )?;
                        assert!(phase.ready(0));
                        phase.edit_slot(|slot| {
                            assert_eq!(slot.count, 1);
                            assert_eq!(slot.hash, 0xfeed_face);
                            assert_eq!(slot.rules[0].elem_hash, 0x1234_5678);
                            assert_eq!(slot.rules[0].decl.weight_bold, 1);
                        })?;
                        assert!(a.store.end_write());
                        Ok(())
                    })?
                })
            })
            .unwrap();
        assert_eq!(cache.data, std::vec![0x5a; 1000]);
        assert_eq!(loan.returns, 1);
        assert!(storage.styles_ready(0));
    }
}
