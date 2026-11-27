#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Native no_std boundary; link native/expat_bridge.c and pinned vendor Expat.
//! See SAFETY.md for the synchronous callback and manual ABI contract.
//! Expat allocates internally. This adapter is NOT an allocation-free XML parser.
use core::ffi::{CStr, c_char, c_int, c_void};
use newt_core::xml_session::{
    Attributes, Handler, ParseObservation, ParserBackend, ProducerBackend,
};
type StartFn = unsafe extern "C" fn(*mut c_void, *const c_char, *const *const c_char);
type EndFn = unsafe extern "C" fn(*mut c_void, *const c_char);
type TextFn = unsafe extern "C" fn(*mut c_void, *const c_char, c_int);
unsafe extern "C" {
    fn newt_xml_create_fail() -> *mut c_void;
    fn newt_xml_create() -> *mut c_void;
    fn newt_xml_producer(parser: *mut c_void);
    fn newt_xml_index(parser: *mut c_void) -> i64;
    fn newt_xml_close(parser: *mut c_void);
    fn newt_xml_feed(
        parser: *mut c_void,
        bytes: *const c_char,
        len: c_int,
        final_chunk: c_int,
        context: *mut c_void,
        start: StartFn,
        end: EndFn,
        text: TextFn,
    ) -> c_int;
    fn newt_xml_feed_observed(
        parser: *mut c_void,
        bytes: *const c_char,
        len: c_int,
        final_chunk: c_int,
        context: *mut c_void,
        start: StartFn,
        end: EndFn,
        text: TextFn,
        root_index: *mut i64,
        cdata_depth: *mut u32,
    ) -> c_int;
}
#[derive(Default)]
pub struct Expat {
    parser: Option<core::ptr::NonNull<c_void>>,
    pub fail_create: bool,
    #[cfg(test)]
    pub created: usize,
    #[cfg(test)]
    pub closed: usize,
}
struct Context<'a> {
    handler: &'a mut dyn Handler,
}
struct NativeAttributes {
    cursor: *const *const c_char,
    base: *const *const c_char,
}
impl NativeAttributes {
    fn lookup(&self, wanted: &[u8], local: bool) -> Option<&[u8]> {
        // Expat guarantees a complete null-terminated pair array for this callback.
        let mut cursor = self.base;
        unsafe {
            while !(*cursor).is_null() {
                let name = CStr::from_ptr(*cursor).to_bytes();
                let matches = if local {
                    newt_core::xml_input::local_is(name, wanted)
                } else {
                    name == newt_core::xml_input::c_bytes(wanted)
                };
                if matches {
                    return Some(CStr::from_ptr(*cursor.add(1)).to_bytes());
                }
                cursor = cursor.add(2);
            }
        }
        None
    }
}
impl Attributes for NativeAttributes {
    fn local(&self, name: &[u8]) -> Option<&[u8]> {
        self.lookup(name, true)
    }
    fn exact(&self, name: &[u8]) -> Option<&[u8]> {
        self.lookup(name, false)
    }
    fn next(&mut self) -> Option<(&[u8], &[u8])> {
        // Expat supplies a null-terminated array of complete name/value pairs;
        // these C strings are live until this synchronous callback returns.
        unsafe {
            let name = *self.cursor;
            if name.is_null() {
                return None;
            }
            let value = *self.cursor.add(1);
            self.cursor = self.cursor.add(2);
            Some((
                CStr::from_ptr(name).to_bytes(),
                CStr::from_ptr(value).to_bytes(),
            ))
        }
    }
}
unsafe extern "C" fn start(ctx: *mut c_void, name: *const c_char, attrs: *const *const c_char) {
    // Only feed installs this exclusive stack context; shim removes it before return.
    let ctx = unsafe { &mut *ctx.cast::<Context<'_>>() };
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    ctx.handler.start_element(
        name,
        &mut NativeAttributes {
            cursor: attrs,
            base: attrs,
        },
    );
}
unsafe extern "C" fn end(ctx: *mut c_void, name: *const c_char) {
    let ctx = unsafe { &mut *ctx.cast::<Context<'_>>() };
    ctx.handler
        .end_element(unsafe { CStr::from_ptr(name) }.to_bytes());
}
unsafe extern "C" fn text(ctx: *mut c_void, bytes: *const c_char, len: c_int) {
    let ctx = unsafe { &mut *ctx.cast::<Context<'_>>() };
    ctx.handler
        .text(unsafe { core::slice::from_raw_parts(bytes.cast::<u8>(), len as usize) });
}
impl Expat {
    fn feed_with(
        &mut self,
        bytes: &[u8],
        final_chunk: bool,
        handler: &mut dyn Handler,
        obs: Option<&mut ParseObservation>,
    ) -> bool {
        let Some(parser) = self.parser else {
            return false;
        };
        let Ok(len) = c_int::try_from(bytes.len()) else {
            return false;
        };
        let mut context = Context { handler };
        // SAFETY: Parser is uniquely owned and non-reentrant. Expat callbacks
        // are synchronous, cannot retain the handler or any borrowed byte view,
        // and all callback/observation pointers are cleared before return.
        unsafe {
            let ok = match obs {
                None => newt_xml_feed(
                    parser.as_ptr(),
                    bytes.as_ptr().cast(),
                    len,
                    c_int::from(final_chunk),
                    (&mut context as *mut Context<'_>).cast(),
                    start,
                    end,
                    text,
                ),
                Some(observation) => newt_xml_feed_observed(
                    parser.as_ptr(),
                    bytes.as_ptr().cast(),
                    len,
                    c_int::from(final_chunk),
                    (&mut context as *mut Context<'_>).cast(),
                    start,
                    end,
                    text,
                    &mut observation.root_index,
                    &mut observation.cdata_depth,
                ),
            };
            ok != 0
        }
    }
}
impl ParserBackend for Expat {
    fn producer_backend(&mut self) -> Option<&mut dyn ProducerBackend> { Some(self) }
    fn create(&mut self) -> bool {
        self.close();
        self.parser = core::ptr::NonNull::new(unsafe {
            if self.fail_create {
                newt_xml_create_fail()
            } else {
                newt_xml_create()
            }
        });
        #[cfg(test)]
        if self.parser.is_some() {
            self.created += 1;
        }
        self.parser.is_some()
    }
    fn feed(&mut self, bytes: &[u8], final_chunk: bool, handler: &mut dyn Handler) -> bool {
        self.feed_with(bytes, final_chunk, handler, None)
    }
    fn close(&mut self) {
        if let Some(parser) = self.parser.take() {
            unsafe { newt_xml_close(parser.as_ptr()) };
            #[cfg(test)]
            {
                self.closed += 1;
            }
        }
    }
}
impl Drop for Expat {
    fn drop(&mut self) {
        self.close();
    }
}

impl ProducerBackend for Expat {
    fn create_producer(&mut self) -> bool {
        if !self.create() {
            return false;
        }
        unsafe { newt_xml_producer(self.parser.expect("created parser").as_ptr()) };
        true
    }
    fn current_byte_index(&self) -> i64 {
        self.parser
            .map_or(-1, |p| unsafe { newt_xml_index(p.as_ptr()) })
    }
    fn feed_observed(
        &mut self,
        bytes: &[u8],
        final_chunk: bool,
        handler: &mut dyn Handler,
        observation: &mut ParseObservation,
    ) -> Option<bool> {
        Some(self.feed_with(bytes, final_chunk, handler, Some(observation)))
    }
}

pub mod probe;
