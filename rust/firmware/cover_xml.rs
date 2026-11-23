// SPDX-License-Identifier: MIT
//! Cover XML passes over Salamander's Expat binding (newt_native_io::cover::xml).
use newt_core::{cover::{MetadataStatus as S, xml::{self,Found,Kind,State}}, xml_session::{Attributes,Handler}};
use newt_core::zip_window::source::BookSource;
use newt_native_io::{UiIo, cover::xml::{self as expat, Allocator, Events, Fault}};
use super::memory::Memory;
// SAFETY: Memory hands out zeroed max-aligned payloads of at least `bytes`,
// resizes as realloc does and releases only its own payloads.
unsafe impl Allocator for Memory<'_> {
    fn allocate(&self, bytes: usize) -> *mut u8 { Memory::allocate(self, bytes) }
    // SAFETY: the trait's contract is Memory::resize's: a live payload of THIS Memory, no loans.
    unsafe fn resize(&self, pointer: *mut u8, bytes: usize) -> *mut u8 {
        // SAFETY: the caller keeps Memory::resize's contract.
        unsafe { Memory::resize(self, pointer, bytes) }
    }
    // SAFETY: the trait's contract is Memory::release's: a payload of THIS Memory, released once.
    unsafe fn release(&self, pointer: *mut u8) {
        // SAFETY: the caller keeps Memory::release's contract.
        unsafe { Memory::release(self, pointer) }
    }
}
/// Expat's events, as the cover XML state takes them.
struct Cover<'a,'b>(&'a mut State<'b>);
impl Handler for Cover<'_,'_> {
    fn start_element(&mut self,name:&[u8],attributes:&mut dyn Attributes) { self.0.start_element(name,attributes) }
    fn end_element(&mut self,name:&[u8]) { self.0.end_element(name) }
    fn text(&mut self,text:&[u8]) { self.0.text(text) }
    fn stop_parse(&self)->bool { self.0.stop_parse() }
}
impl Events for Cover<'_,'_> {
    // A refused DOCTYPE stops before its internal subset is read.
    fn doctype(&mut self,subset:bool) { self.0.doctype(subset) }
    fn external(&mut self) { self.0.reject() }
}
fn parse(io:&UiIo,raw:&mut dyn BookSource,memory:&Memory<'_>,state:&mut State<'_>)->S {
    match expat::parse(io,raw,memory,&mut Cover(state)) {
        Ok(()) => {}
        Err(Fault::Refused(status)) => return status,
        Err(Fault::Read) => state.status=S::IoError,
        // A callback's own refusal stands over the stop it caused.
        Err(Fault::Parse(status)) => if state.status==S::Ok { state.status=status },
    }
    state.complete()
}
/// Each parse borrows a scratch block from cover Memory, so no path array
/// sits on the UI stack under Expat's callbacks.
fn with_state(memory:&Memory<'_>,kind:Kind,opf:&[u8],found:&mut Found,run:impl FnOnce(&mut State<'_>)->S)->S{
    let Some(mut block)=memory.block(xml::SCRATCH) else { return S::OutOfMemory; };
    let Some(mut state)=State::new(kind,opf,found,block.as_mut()) else { return S::OutOfMemory; };
    run(&mut state)
}
/// The package path goes to `found.image`.
pub fn container(io:&UiIo,raw:&mut dyn BookSource,memory:&Memory<'_>,found:&mut Found)->S {
    let result=with_state(memory,Kind::Container,b"",found,|state| parse(io,raw,memory,state));
    if result==S::Ok && found.image[0]==0 { S::NotFound } else { result }
}
pub fn package(io:&UiIo,raw:&mut dyn BookSource,memory:&Memory<'_>,opf:&[u8],found:&mut Found)->S {
    use newt_core::cover::discovery::{path,cbytes};
    let mut canonical=[0;512];
    if !path(b"",opf,&mut canonical) || cbytes(opf)!=cbytes(&canonical) { return S::Unsupported; }
    with_state(memory,Kind::Package,opf,found,|state| {
        let mut result=parse(io,raw,memory,state);
        if result==S::Ok && state.second_pass() { state.resolve=true; state.saw_root=false; result=parse(io,raw,memory,state); }
        if result==S::Ok { state.settle() } else { result }
    })
}
/// Reads `found.page`; its cover image replaces `found.image`.
pub fn page(io:&UiIo,raw:&mut dyn BookSource,memory:&Memory<'_>,found:&mut Found)->S {
    with_state(memory,Kind::Page,b"",found,|state| {
        let result=parse(io,raw,memory,state);
        if result==S::Ok { state.take_page_image() } else { result }
    })
}
