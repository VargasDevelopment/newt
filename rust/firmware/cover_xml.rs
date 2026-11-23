// SPDX-License-Identifier: MIT
//! Cover-only actual Expat allocator lease and callbacks, frozen discovery.h.
use core::{ffi::{CStr,c_char,c_int,c_void}, ptr};
use newt_core::{cover::{MetadataStatus as S, read, xml::{self,Found,Kind,State}}, xml_session::{Handler,Attributes}};
use newt_core::zip_window::source::BookSource;
use super::memory::Memory;
#[repr(C)]
struct Suite {
    // SAFETY: Expat invokes only during the exclusive active Memory lease.
    allocate: unsafe extern "C" fn(usize)->*mut c_void,
    // SAFETY: Expat passes only this suite's live payload, with no live loan.
    resize: unsafe extern "C" fn(*mut c_void,usize)->*mut c_void,
    // SAFETY: Expat relinquishes this suite's exact original payload once.
    free: unsafe extern "C" fn(*mut c_void),
}
// SAFETY: serialized UI parser alone owns this lease; nested parses fail Busy.
// Memory and its heap/observation outlive ParserFree and all callbacks, which
// cannot reenter. Erasure extends no lifetime; ParserFree clears the pointer.
static mut XML_MEMORY: *const () = ptr::null();
// SAFETY: actual pinned Expat C ABI, synchronous callbacks and borrowed byte
// extents. No callback or parser pointer survives the enclosing Parser lease.
unsafe extern "C" {
    fn XML_ParserCreate_MM(encoding:*const c_char,suite:*const Suite,separator:*const c_char)->*mut c_void;
    fn XML_ParserFree(parser:*mut c_void);
    fn XML_SetUserData(parser:*mut c_void,user:*mut c_void);
    fn newt_cover_xml_user(parser:*mut c_void)->*mut c_void;
    fn XML_SetElementHandler(parser:*mut c_void,
        // SAFETY: callback strings/pairs and exclusive context live until return.
        start:unsafe extern "C" fn(*mut c_void,*const c_char,*const *const c_char),
        // SAFETY: callback name and exclusive context live until return.
        end:unsafe extern "C" fn(*mut c_void,*const c_char));
    fn XML_SetStartDoctypeDeclHandler(parser:*mut c_void,
        // SAFETY: synchronous callback only rejects this live parser's DTD.
        handler:unsafe extern "C" fn(*mut c_void,*const c_char,*const c_char,*const c_char,c_int));
    fn XML_SetExternalEntityRefHandler(parser:*mut c_void,
        // SAFETY: synchronous callback only rejects this live parser's entity.
        handler:unsafe extern "C" fn(*mut c_void,*const c_char,*const c_char,*const c_char,*const c_char)->c_int);
    fn XML_SetParamEntityParsing(parser:*mut c_void,value:c_int)->c_int;
    fn XML_Parse(parser:*mut c_void,data:*const c_char,bytes:c_int,final_chunk:c_int)->c_int;
    fn XML_GetErrorCode(parser:*mut c_void)->c_int;
    fn XML_StopParser(parser:*mut c_void,resumable:u8)->c_int;
}
// SAFETY: called solely by active Expat with the serialized Memory lease live.
unsafe extern "C" fn allocate(bytes:usize)->*mut c_void {
    // SAFETY: Expat invokes this only during the active serialized Memory lease.
    unsafe { if XML_MEMORY.is_null() { ptr::null_mut() } else { (*XML_MEMORY.cast::<Memory<'_>>()).allocate(bytes).cast() } }
}
// SAFETY: data belongs to this active Expat suite; no consumer retains a loan.
unsafe extern "C" fn resize(data:*mut c_void,bytes:usize)->*mut c_void {
    // SAFETY: Expat transfers only a live allocation from this same active suite.
    unsafe { if XML_MEMORY.is_null() { ptr::null_mut() } else { (*XML_MEMORY.cast::<Memory<'_>>()).resize(data.cast(),bytes).cast() } }
}
// SAFETY: data is null or this active suite's exact original live payload.
unsafe extern "C" fn free(data:*mut c_void) {
    // SAFETY: same suite and original payload; Expat relinquishes all its loans.
    unsafe { if !XML_MEMORY.is_null() { (*XML_MEMORY.cast::<Memory<'_>>()).release(data.cast()); } }
}
struct Context<'a,'b> { state:&'a mut State<'b>, parser:*mut c_void }
struct Attrs { base:*const *const c_char, cursor:*const *const c_char }
impl Attributes for Attrs {
    fn next(&mut self)->Option<(&[u8],&[u8])> {
        // SAFETY: Expat supplies a complete terminated key/value pair array
        // whose C strings remain live until this single callback returns.
        unsafe {
            if (*self.cursor).is_null() { return None; }
            let key=CStr::from_ptr(*self.cursor).to_bytes();
            let value=CStr::from_ptr(*self.cursor.add(1)).to_bytes();
            self.cursor=self.cursor.add(2); Some((key,value))
        }
    }
    fn exact(&self,key:&[u8])->Option<&[u8]> {
        let mut at=self.base;
        // SAFETY: same callback-local complete Expat pair array; no byte escapes.
        unsafe {
            while !(*at).is_null() {
                if CStr::from_ptr(*at).to_bytes()==key { return Some(CStr::from_ptr(*at.add(1)).to_bytes()); }
                at=at.add(2);
            }
        }
        None
    }
    fn local(&self,key:&[u8])->Option<&[u8]> { self.exact(key) }
}
// SAFETY: Expat supplies its live exclusive Context and complete C string pairs.
unsafe extern "C" fn start(user:*mut c_void,name:*const c_char,attrs:*const *const c_char) {
    // SAFETY: parser's exclusive stack context and Expat strings live through
    // this callback; abort only marks this parser, never unwinds through C.
    unsafe {
        let c=&mut *user.cast::<Context<'_,'_>>();
        c.state.start_element(CStr::from_ptr(name).to_bytes(),&mut Attrs{base:attrs,cursor:attrs});
        if c.state.stop_parse() { XML_StopParser(c.parser,0); }
    }
}
// SAFETY: Expat supplies its live exclusive Context and complete name C string.
unsafe extern "C" fn end(user:*mut c_void,name:*const c_char) {
    // SAFETY: same live parser-local context and terminated name.
    unsafe { (&mut *user.cast::<Context<'_,'_>>()).state.end_element(CStr::from_ptr(name).to_bytes()); }
}
// SAFETY: user is this parser's live exclusive Context; only a policy check occurs.
unsafe extern "C" fn doctype(user:*mut c_void,_a:*const c_char,_b:*const c_char,_c:*const c_char,subset:c_int) {
    // SAFETY: synchronous parser context; a refused DOCTYPE stops before its
    // internal subset is read. Parameter entities are off, so no DTD loads.
    unsafe {
        let c=&mut *user.cast::<Context<'_,'_>>(); c.state.doctype(subset!=0);
        if c.state.stop_parse() { XML_StopParser(c.parser,0); }
    }
}
// SAFETY: parser is Expat's active parser with this operation's Context installed.
unsafe extern "C" fn external(parser:*mut c_void,_a:*const c_char,_b:*const c_char,_c:*const c_char,_d:*const c_char)->c_int {
    // SAFETY: actual Expat parser carries the active Context installed below.
    unsafe { let c=&mut *newt_cover_xml_user(parser).cast::<Context<'_,'_>>(); c.state.reject(); XML_StopParser(parser,0); }
    0
}
struct Parser(*mut c_void);
impl Drop for Parser {
    fn drop(&mut self) {
        // SAFETY: unique actual parser, all callbacks/feeds have returned. Free
        // uses the SAME still-live Memory before clearing the global lease.
        unsafe { XML_ParserFree(self.0); XML_MEMORY=ptr::null(); }
    }
}
fn parse(raw:&mut dyn BookSource,memory:&Memory<'_>,state:&mut State<'_>)->S {
    // SAFETY: UI task exclusively serializes cover parsing; nonnull fails Busy.
    if unsafe { !XML_MEMORY.is_null() } { return S::Busy; }
    if raw.size()==0 || raw.size()>256*1024 { return S::Unsupported; }
    let suite=Suite{allocate,resize,free};
    // SAFETY: memory outlives parser; actual Expat copies this suite. Namespace
    // separator is the frozen "}"; no reader parser or callback is retained.
    let pointer=unsafe { XML_MEMORY=(memory as *const Memory<'_>).cast(); XML_ParserCreate_MM(ptr::null(),&suite,c"}".as_ptr()) };
    if pointer.is_null() {
        // SAFETY: failed creation retained no parser/callback or Memory lease.
        unsafe { XML_MEMORY=ptr::null(); } return S::OutOfMemory;
    }
    let parser=Parser(pointer); let mut context=Context{state,parser:pointer};
    // SAFETY: exclusive parser and stack context survive every synchronous feed.
    unsafe {
        XML_SetUserData(pointer,(&mut context as *mut Context<'_,'_>).cast());
        XML_SetElementHandler(pointer,start,end); XML_SetStartDoctypeDeclHandler(pointer,doctype);
        XML_SetExternalEntityRefHandler(pointer,external); XML_SetParamEntityParsing(pointer,0);
    }
    let mut chunk=[0;1024]; let mut at=0;
    while at<raw.size() {
        let take=chunk.len().min((raw.size()-at) as usize);
        if !read(raw,at,&mut chunk[..take]) { context.state.status=S::IoError; break; }
        at+=take as u64;
        // SAFETY: initialized exact byte prefix, parser/context remain live.
        if unsafe { XML_Parse(pointer,chunk.as_ptr().cast(),take as c_int,c_int::from(at==raw.size())) }==0 {
            if context.state.status==S::Ok {
                // SAFETY: actual error of this live parser, no pointer retained.
                context.state.status=if unsafe { XML_GetErrorCode(pointer) }==1 { S::OutOfMemory } else { S::Malformed };
            }
            break;
        }
    }
    drop(parser); context.state.complete()
}
/// Each parse borrows a scratch block from cover Memory, so no path array
/// sits on the UI stack under Expat's callbacks.
fn with_state(memory:&Memory<'_>,kind:Kind,opf:&[u8],found:&mut Found,run:impl FnOnce(&mut State<'_>)->S)->S{
    let Some(mut block)=memory.block(xml::SCRATCH) else { return S::OutOfMemory; };
    let Some(mut state)=State::new(kind,opf,found,block.as_mut()) else { return S::OutOfMemory; };
    run(&mut state)
}
/// The package path goes to `found.image`.
pub fn container(raw:&mut dyn BookSource,memory:&Memory<'_>,found:&mut Found)->S {
    let result=with_state(memory,Kind::Container,b"",found,|state| parse(raw,memory,state));
    if result==S::Ok && found.image[0]==0 { S::NotFound } else { result }
}
pub fn package(raw:&mut dyn BookSource,memory:&Memory<'_>,opf:&[u8],found:&mut Found)->S {
    use newt_core::cover::discovery::{path,cbytes};
    let mut canonical=[0;512];
    if !path(b"",opf,&mut canonical) || cbytes(opf)!=cbytes(&canonical) { return S::Unsupported; }
    with_state(memory,Kind::Package,opf,found,|state| {
        let mut result=parse(raw,memory,state);
        if result==S::Ok && state.second_pass() { state.resolve=true; state.saw_root=false; result=parse(raw,memory,state); }
        if result==S::Ok { state.settle() } else { result }
    })
}
/// Reads `found.page`; its cover image replaces `found.image`.
pub fn page(raw:&mut dyn BookSource,memory:&Memory<'_>,found:&mut Found)->S {
    with_state(memory,Kind::Page,b"",found,|state| {
        let result=parse(raw,memory,state);
        if result==S::Ok { state.take_page_image() } else { result }
    })
}
