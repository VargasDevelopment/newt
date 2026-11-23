// SPDX-License-Identifier: MIT
//! The cover parser's Expat binding, frozen discovery.h: the pinned C ABI, its
//! allocator suite and callbacks. A parse leases the caller's cover allocator
//! and forwards Expat's events; the cover policy stays with the caller.
use core::{ffi::{CStr,c_char,c_int,c_void}, ptr};
use newt_core::{cover::{MetadataStatus as S, read}, xml_session::{Attributes,Handler}};
use newt_core::zip_window::source::BookSource;
use crate::UiIo;
/// A cover parse's events: Handler's, plus the DOCTYPE and external entity
/// reports. Expat stops once `stop_parse` holds after a start or a DOCTYPE.
pub trait Events: Handler {
    /// A DOCTYPE; `subset` says whether it has an internal subset.
    fn doctype(&mut self, subset: bool);
    /// An external entity reference. Expat stops after it.
    fn external(&mut self);
}
/// Why a parse ended early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// No parse ran: Busy (another parse is live), Unsupported (empty or over
    /// 256 KiB) or OutOfMemory (no parser).
    Refused(S),
    /// A read of the source failed.
    Read,
    /// Expat stopped: OutOfMemory for its no-memory error, else Malformed.
    Parse(S),
}
/// Every Expat allocation of a parse goes through this, and Expat writes
/// through what it returns.
///
/// # Safety
/// As C's malloc family: `allocate` returns null or a writable block of at
/// least `bytes`, aligned for any C type. `resize` keeps realloc's meaning:
/// null or a block of at least `bytes` holding the old contents, the old block
/// released on success and kept on failure. `release` frees a block from this
/// allocator once and ignores null.
pub unsafe trait Allocator {
    fn allocate(&self, bytes: usize) -> *mut u8;
    /// # Safety
    /// `pointer` is null or a live payload from this allocator with no loans.
    unsafe fn resize(&self, pointer: *mut u8, bytes: usize) -> *mut u8;
    /// # Safety
    /// `pointer` is null or a live payload from this allocator, released once.
    unsafe fn release(&self, pointer: *mut u8);
}
#[repr(C)]
struct Suite {
    // SAFETY: Expat invokes only during the exclusive active allocator lease.
    allocate: unsafe extern "C" fn(usize)->*mut c_void,
    // SAFETY: Expat passes only this suite's live payload, with no live loan.
    resize: unsafe extern "C" fn(*mut c_void,usize)->*mut c_void,
    // SAFETY: Expat relinquishes this suite's exact original payload once.
    free: unsafe extern "C" fn(*mut c_void),
}
// SAFETY: only parse touches this lease, and parse needs the UI owner's
// `UiIo`, so no other task races it; a nested parse fails Busy. It points at
// parse's local `&dyn Allocator`, which outlives ParserFree and all callbacks,
// which cannot reenter. ParserFree clears the pointer.
static mut ALLOCATOR: *const () = ptr::null();
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
    fn XML_SetCharacterDataHandler(parser:*mut c_void,
        // SAFETY: callback bytes and exclusive context live until return.
        handler:unsafe extern "C" fn(*mut c_void,*const c_char,c_int));
    fn XML_SetExternalEntityRefHandler(parser:*mut c_void,
        // SAFETY: synchronous callback only rejects this live parser's entity.
        handler:unsafe extern "C" fn(*mut c_void,*const c_char,*const c_char,*const c_char,*const c_char)->c_int);
    fn XML_SetParamEntityParsing(parser:*mut c_void,value:c_int)->c_int;
    fn XML_Parse(parser:*mut c_void,data:*const c_char,bytes:c_int,final_chunk:c_int)->c_int;
    fn XML_GetErrorCode(parser:*mut c_void)->c_int;
    fn XML_StopParser(parser:*mut c_void,resumable:u8)->c_int;
}
// SAFETY: called solely by active Expat with the serialized allocator lease live.
unsafe extern "C" fn allocate(bytes:usize)->*mut c_void {
    // SAFETY: Expat invokes this only during the active serialized lease.
    unsafe { if ALLOCATOR.is_null() { ptr::null_mut() } else { (*ALLOCATOR.cast::<&dyn Allocator>()).allocate(bytes).cast() } }
}
// SAFETY: data belongs to this active Expat suite; no consumer retains a loan.
unsafe extern "C" fn resize(data:*mut c_void,bytes:usize)->*mut c_void {
    // SAFETY: Expat transfers only a live allocation from this same active suite.
    unsafe { if ALLOCATOR.is_null() { ptr::null_mut() } else { (*ALLOCATOR.cast::<&dyn Allocator>()).resize(data.cast(),bytes).cast() } }
}
// SAFETY: data is null or this active suite's exact original live payload.
unsafe extern "C" fn free(data:*mut c_void) {
    // SAFETY: same suite and original payload; Expat relinquishes all its loans.
    unsafe { if !ALLOCATOR.is_null() { (*ALLOCATOR.cast::<&dyn Allocator>()).release(data.cast()); } }
}
struct Context<'a> { events:&'a mut dyn Events, parser:*mut c_void }
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
        let c=&mut *user.cast::<Context<'_>>();
        c.events.start_element(CStr::from_ptr(name).to_bytes(),&mut Attrs{base:attrs,cursor:attrs});
        if c.events.stop_parse() { XML_StopParser(c.parser,0); }
    }
}
// SAFETY: Expat supplies its live exclusive Context and complete name C string.
unsafe extern "C" fn end(user:*mut c_void,name:*const c_char) {
    // SAFETY: same live parser-local context and terminated name.
    unsafe { (&mut *user.cast::<Context<'_>>()).events.end_element(CStr::from_ptr(name).to_bytes()); }
}
// SAFETY: Expat supplies its live exclusive Context and a byte extent it owns.
unsafe extern "C" fn text(user:*mut c_void,bytes:*const c_char,length:c_int) {
    // SAFETY: same live parser-local context; Expat's extent lives until return.
    unsafe {
        let c=&mut *user.cast::<Context<'_>>();
        c.events.text(core::slice::from_raw_parts(bytes.cast(),usize::try_from(length).unwrap_or(0)));
    }
}
// SAFETY: user is this parser's live exclusive Context; only a policy check occurs.
unsafe extern "C" fn doctype(user:*mut c_void,_a:*const c_char,_b:*const c_char,_c:*const c_char,subset:c_int) {
    // SAFETY: synchronous parser context; a refused DOCTYPE stops before its
    // internal subset is read. Parameter entities are off, so no DTD loads.
    unsafe {
        let c=&mut *user.cast::<Context<'_>>(); c.events.doctype(subset!=0);
        if c.events.stop_parse() { XML_StopParser(c.parser,0); }
    }
}
// SAFETY: parser is Expat's active parser with this operation's Context installed.
unsafe extern "C" fn external(parser:*mut c_void,_a:*const c_char,_b:*const c_char,_c:*const c_char,_d:*const c_char)->c_int {
    // SAFETY: actual Expat parser carries the active Context installed below.
    unsafe { let c=&mut *newt_cover_xml_user(parser).cast::<Context<'_>>(); c.events.external(); XML_StopParser(parser,0); }
    0
}
struct Parser(*mut c_void);
impl Drop for Parser {
    fn drop(&mut self) {
        // SAFETY: unique actual parser, all callbacks/feeds have returned. Free
        // uses the SAME still-live allocator before clearing the global lease.
        unsafe { XML_ParserFree(self.0); ALLOCATOR=ptr::null(); }
    }
}
/// Parses all of `raw` into `events`, with every Expat allocation from
/// `allocator`. `_io` is the UI owner's authority, which serializes parses.
pub fn parse(_io:&UiIo,raw:&mut dyn BookSource,allocator:&dyn Allocator,events:&mut dyn Events)->Result<(),Fault> {
    // SAFETY: `_io` keeps every parse on the one UI owner; nonnull fails Busy.
    if unsafe { !ALLOCATOR.is_null() } { return Err(Fault::Refused(S::Busy)); }
    if raw.size()==0 || raw.size()>256*1024 { return Err(Fault::Refused(S::Unsupported)); }
    let suite=Suite{allocate,resize,free};
    // SAFETY: `allocator` outlives parser; actual Expat copies this suite. Namespace
    // separator is the frozen "}"; no reader parser or callback is retained.
    let pointer=unsafe { ALLOCATOR=(&allocator as *const &dyn Allocator).cast(); XML_ParserCreate_MM(ptr::null(),&suite,c"}".as_ptr()) };
    if pointer.is_null() {
        // SAFETY: failed creation retained no parser/callback or allocator lease.
        unsafe { ALLOCATOR=ptr::null(); } return Err(Fault::Refused(S::OutOfMemory));
    }
    let parser=Parser(pointer); let mut context=Context{events,parser:pointer};
    // SAFETY: exclusive parser and stack context survive every synchronous feed.
    unsafe {
        XML_SetUserData(pointer,(&mut context as *mut Context<'_>).cast());
        XML_SetElementHandler(pointer,start,end); XML_SetCharacterDataHandler(pointer,text);
        XML_SetStartDoctypeDeclHandler(pointer,doctype);
        XML_SetExternalEntityRefHandler(pointer,external); XML_SetParamEntityParsing(pointer,0);
    }
    let mut chunk=[0;1024]; let mut at=0;
    while at<raw.size() {
        let take=chunk.len().min((raw.size()-at) as usize);
        if !read(raw,at,&mut chunk[..take]) { return Err(Fault::Read); }
        at+=take as u64;
        // SAFETY: initialized exact byte prefix, parser/context remain live.
        if unsafe { XML_Parse(pointer,chunk.as_ptr().cast(),take as c_int,c_int::from(at==raw.size())) }==0 {
            // SAFETY: actual error of this live parser, no pointer retained.
            return Err(Fault::Parse(if unsafe { XML_GetErrorCode(pointer) }==1 { S::OutOfMemory } else { S::Malformed }));
        }
    }
    drop(parser); Ok(())
}
