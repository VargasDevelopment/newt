//! Auditable unsafe boundary to vendored FreeType C, shared by the host proof
//! and no_std firmware. Target use is unaccepted until SDK compile/link review.
use core::{
    cell::Cell,
    ffi::{c_ulong, c_void},
    marker::PhantomData,
    ptr::NonNull,
};
use newt_core::{ft_font::*, zip_window::source::BookSource};
#[repr(C)]
pub struct MemoryCallbacks {
    pub context: *mut c_void,
    pub allocate: unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void,
    pub deallocate: unsafe extern "C" fn(*mut c_void, *mut c_void),
    pub reallocate: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, usize) -> *mut c_void,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct RawAxis {
    tag: u32,
    minimum: i32,
    def: i32,
    maximum: i32,
}
#[repr(C)]
#[derive(Default)]
struct RawSlot {
    outline: i32,
    bitmap: i32,
    linear_advance: i64,
    advance: i64,
    left: i64,
    top: i64,
    width: u32,
    rows: u32,
    pitch: i32,
    mono: i32,
    xmin: i64,
    xmax: i64,
    ymin: i64,
    ymax: i64,
}
#[repr(C)]
struct RawLines {
    ascender: i32,
    descender: i32,
    height: i32,
}
// Evidence for these asserted layouts under each recorded configuration;
// calling convention, final linkage and runtime behavior need separate proof.
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    assert!(size_of::<MemoryCallbacks>() == 4 * size_of::<usize>());
    assert!(align_of::<MemoryCallbacks>() == align_of::<usize>());
    assert!(offset_of!(MemoryCallbacks, context) == 0);
    assert!(offset_of!(MemoryCallbacks, allocate) == size_of::<usize>());
    assert!(offset_of!(MemoryCallbacks, deallocate) == 2 * size_of::<usize>());
    assert!(offset_of!(MemoryCallbacks, reallocate) == 3 * size_of::<usize>());
    assert!(align_of::<RawAxis>() == 4 && align_of::<RawLines>() == 4);
    assert!(align_of::<RawSlot>() == 8);
    #[cfg(target_pointer_width = "32")]
    assert!(size_of::<usize>() == 4 && size_of::<c_ulong>() == 4);
    assert!(size_of::<RawAxis>() == 16);
    assert!(size_of::<RawLines>() == 12);
    assert!(size_of::<RawSlot>() == 88);
    assert!(offset_of!(RawSlot, linear_advance) == 8);
    assert!(offset_of!(RawSlot, width) == 40);
    assert!(offset_of!(RawSlot, xmin) == 56);
};
type ReadFn = unsafe extern "C" fn(*mut c_void, c_ulong, *mut u8, c_ulong) -> c_ulong;
unsafe extern "C" {
    fn nf_library_new(m: *const MemoryCallbacks, e: *mut i32) -> *mut c_void;
    fn nf_library_done(l: *mut c_void);
    fn nf_capabilities() -> u32;
    fn nf_open(
        l: *mut c_void,
        b: *const u8,
        n: usize,
        read: Option<ReadFn>,
        ctx: *mut c_void,
        size: c_ulong,
        stage: *mut i32,
        e: *mut i32,
    ) -> *mut c_void;
    fn nf_close(f: *mut c_void);
    fn nf_axes(f: *mut c_void, out: *mut RawAxis) -> usize;
    fn nf_coordinates(f: *mut c_void, c: *const i32, n: usize);
    fn nf_size(f: *mut c_void, n: u32) -> i32;
    fn nf_glyph(f: *mut c_void, cp: u32) -> u32;
    fn nf_properties(f: *mut c_void, dark: i32, version: u8);
    fn nf_transform(f: *mut c_void, shear: i32);
    fn nf_load(f: *mut c_void, g: u32, flags: i32) -> i32;
    fn nf_embolden(f: *mut c_void, s: i32) -> i32;
    fn nf_render(f: *mut c_void, mono: i32) -> i32;
    fn nf_get_slot(f: *mut c_void) -> RawSlot;
    fn nf_bitmap(f: *mut c_void, n: *mut usize) -> *const u8;
    fn nf_line_metrics(f: *mut c_void) -> RawLines;
    fn nf_kern(f: *mut c_void, l: u32, r: u32) -> i32;
    fn nf_scale(f: *mut c_void, u: i32) -> i32;
    fn nf_table(f: *mut c_void, tag: u32, b: *mut u8, n: *mut usize) -> i32;
    fn nf_describe(
        f: *mut c_void,
        b: *mut u8,
        n: usize,
        len: *mut usize,
        w: *mut u16,
        it: *mut i32,
    ) -> i32;
    fn nf_stream_failed(f: *mut c_void) -> i32;
}
#[cfg(target_os = "none")]
unsafe extern "C" {
    /// Static, context-free callbacks over pinned FreeInkFont FontAlloc.c.
    fn nf_font_heap() -> *const MemoryCallbacks;
}
/// One caller-owned library, shared among faces on one thread. Properties are
/// applied before each load. No global Rust cache, refcount or interior face state.
pub struct Library {
    raw: NonNull<c_void>,
    _single_thread: PhantomData<Cell<()>>,
}
impl Library {
    /// Host libc default. Absent on target: the native C rejects null there.
    #[cfg(not(target_os = "none"))]
    pub fn host() -> Result<Self, i32> {
        unsafe { Self::create(core::ptr::null()) }
    }
    /// FtFont's own default allocator, fiFontMalloc/Realloc/Free (C3: internal
    /// heap; no PSRAM). Allocates only the library here; faces allocate on open.
    /// # Safety
    /// The SDK heap is initialized and the caller runs in ordinary task context
    /// (no ISR, early startup or shutdown). Nothing in Rust can witness this.
    #[cfg(target_os = "none")]
    pub unsafe fn font_heap() -> Result<Self, i32> {
        unsafe { Self::create(nf_font_heap()) }
    }
    /// # Safety
    /// Callbacks must obey the C allocator contract and their context must remain
    /// alive until all faces and this library are dropped. No callback may unwind.
    pub unsafe fn with_callbacks(m: &MemoryCallbacks) -> Result<Self, i32> {
        unsafe { Self::create(m) }
    }
    unsafe fn create(m: *const MemoryCallbacks) -> Result<Self, i32> {
        let mut e = 0;
        let raw = unsafe { nf_library_new(m, &mut e) };
        NonNull::new(raw)
            .map(|raw| Self {
                raw,
                _single_thread: PhantomData,
            })
            .ok_or(e)
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            nf_library_done(self.raw.as_ptr());
        }
    }
}
pub trait ReadAt {
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> usize;
}
/// Borrowed BookSource (e.g. FileBookSource) as a font stream. One callback
/// makes one source read, as pinned ftStreamIo does. A short result reaches
/// the native hook unchanged and latches failure; invalid counts return zero.
pub struct BookStream<'s, S: BookSource + ?Sized> {
    source: &'s mut S,
}
impl<'s, S: BookSource + ?Sized> BookStream<'s, S> {
    pub fn new(source: &'s mut S) -> Self {
        Self { source }
    }
    pub fn length(&self) -> u64 {
        self.source.size()
    }
}
impl<S: BookSource + ?Sized> ReadAt for BookStream<'_, S> {
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> usize {
        if output.is_empty() {
            return 0;
        }
        usize::try_from(self.source.read_at(offset, output))
            .ok()
            .filter(|&count| count <= output.len())
            .unwrap_or(0)
    }
}
/// Caller stack storage: create before Native, never move while borrowed.
pub struct StreamContext<'a> {
    pub reader: &'a mut dyn ReadAt,
    pub length: u64,
}
unsafe extern "C" fn stream_read(
    ctx: *mut c_void,
    offset: c_ulong,
    b: *mut u8,
    n: c_ulong,
) -> c_ulong {
    // C invokes only with a valid output for count>0, synchronously, without aliasing.
    if n == 0 {
        return 0;
    }
    let context = unsafe { &mut *ctx.cast::<StreamContext<'_>>() };
    // FreeType also supplies uninitialized stack/FT_QALLOC storage. Initialize
    // before constructing a safe byte slice that ReadAt may inspect.
    unsafe { core::ptr::write_bytes(b, 0, n as usize) };
    let output = unsafe { core::slice::from_raw_parts_mut(b, n as usize) };
    context
        .reader
        .read_at(offset as u64, output)
        .min(output.len()) as c_ulong
}
pub struct Native<'l, 'a> {
    library: &'l Library,
    face: Option<NonNull<c_void>>,
    stream: *mut c_void,
    length: c_ulong,
    failed: bool,
    _source: PhantomData<&'a mut StreamContext<'a>>,
}
impl<'l, 'a> Native<'l, 'a> {
    pub fn memory(library: &'l Library) -> Self {
        Self {
            library,
            face: None,
            stream: core::ptr::null_mut(),
            length: 0,
            failed: false,
            _source: PhantomData,
        }
    }
    pub fn stream(library: &'l Library, ctx: &'a mut StreamContext<'a>) -> Self {
        let length = c_ulong::try_from(ctx.length).unwrap_or(0);
        Self {
            library,
            face: None,
            stream: (ctx as *mut StreamContext<'a>).cast(),
            length,
            failed: false,
            _source: PhantomData,
        }
    }
    fn raw(&self) -> *mut c_void {
        self.face.map_or(core::ptr::null_mut(), NonNull::as_ptr)
    }
}
fn result(e: i32) -> Result<(), i32> {
    if e == 0 { Ok(()) } else { Err(e) }
}
impl<'l, 'a> Backend<'a> for Native<'l, 'a> {
    fn open(&mut self, memory: Option<&'a [u8]>) -> Result<(), OpenError> {
        self.close();
        self.failed = false;
        let (mut stage, mut e) = (0, 0);
        let (b, n, read, ctx, len) = match memory {
            Some(b) => (b.as_ptr(), b.len(), None, core::ptr::null_mut(), 0),
            None => (
                core::ptr::null(),
                0,
                if self.stream.is_null() {
                    None
                } else {
                    Some(stream_read as ReadFn)
                },
                self.stream,
                self.length,
            ),
        };
        let p = unsafe {
            nf_open(
                self.library.raw.as_ptr(),
                b,
                n,
                read,
                ctx,
                len,
                &mut stage,
                &mut e,
            )
        };
        self.face = NonNull::new(p);
        if self.face.is_some() {
            Ok(())
        } else {
            self.failed = stage == 6;
            Err(OpenError {
                stage: match stage {
                    2 => InitFailure::Source,
                    3 => InitFailure::Allocation,
                    _ => InitFailure::OpenFace,
                },
                raw: e,
            })
        }
    }
    fn close(&mut self) {
        if let Some(f) = self.face.take() {
            unsafe {
                self.failed |= nf_stream_failed(f.as_ptr()) != 0;
                nf_close(f.as_ptr());
            }
        }
    }
    fn capabilities(&self) -> Capabilities {
        let c = unsafe { nf_capabilities() };
        Capabilities {
            auto: c & 1 != 0,
            native: c & 2 != 0,
            mono: c & 4 != 0,
        }
    }
    fn axes(&mut self, out: &mut [Axis; 16]) -> usize {
        let mut a = [RawAxis::default(); 16];
        let n = unsafe { nf_axes(self.raw(), a.as_mut_ptr()) }.min(16);
        for i in 0..n {
            out[i] = Axis {
                tag: a[i].tag,
                minimum: a[i].minimum,
                default: a[i].def,
                maximum: a[i].maximum,
            };
        }
        n
    }
    fn set_coordinates(&mut self, c: &[i32]) {
        unsafe {
            nf_coordinates(self.raw(), c.as_ptr(), c.len());
        }
    }
    fn set_size(&mut self, n: u32) -> Result<(), i32> {
        result(unsafe { nf_size(self.raw(), n) })
    }
    fn glyph_id(&self, cp: u32) -> u32 {
        unsafe { nf_glyph(self.raw(), cp) }
    }
    fn properties(&mut self, d: bool, v: u8) {
        unsafe {
            nf_properties(self.raw(), i32::from(d), v);
        }
    }
    fn transform(&mut self, s: i32) {
        unsafe {
            nf_transform(self.raw(), s);
        }
    }
    fn load(&mut self, g: u32, f: i32) -> Result<(), i32> {
        result(unsafe { nf_load(self.raw(), g, f) })
    }
    fn embolden(&mut self, s: i32) -> Result<(), i32> {
        result(unsafe { nf_embolden(self.raw(), s) })
    }
    fn render(&mut self, m: bool) -> Result<(), i32> {
        result(unsafe { nf_render(self.raw(), i32::from(m)) })
    }
    fn slot(&self) -> Slot {
        let r = unsafe { nf_get_slot(self.raw()) };
        Slot {
            outline: r.outline != 0,
            bitmap: r.bitmap != 0,
            linear_advance: r.linear_advance,
            advance: r.advance,
            left: r.left,
            top: r.top,
            width: r.width,
            rows: r.rows,
            pitch: r.pitch,
            mono: r.mono != 0,
            xmin: r.xmin,
            xmax: r.xmax,
            ymin: r.ymin,
            ymax: r.ymax,
        }
    }
    fn bitmap(&self) -> &[u8] {
        let mut n = 0;
        let p = unsafe { nf_bitmap(self.raw(), &mut n) };
        if n == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(p, n) }
        }
    }
    fn line_metrics(&self) -> LineMetrics {
        let m = unsafe { nf_line_metrics(self.raw()) };
        LineMetrics {
            ascender26_6: m.ascender,
            descender26_6: m.descender,
            height26_6: m.height,
        }
    }
    fn legacy_kern(&mut self, l: u32, r: u32) -> i32 {
        unsafe { nf_kern(self.raw(), l, r) }
    }
    fn scale_x(&self, u: i32) -> i32 {
        unsafe { nf_scale(self.raw(), u) }
    }
    fn table_length(&mut self, tag: u32) -> Option<usize> {
        let mut n = 0;
        if unsafe { nf_table(self.raw(), tag, core::ptr::null_mut(), &mut n) } == 0 {
            Some(n)
        } else {
            None
        }
    }
    fn read_table(&mut self, tag: u32, b: &mut [u8]) -> bool {
        let mut n = b.len();
        unsafe { nf_table(self.raw(), tag, b.as_mut_ptr(), &mut n) == 0 && n == b.len() }
    }
    fn describe(&mut self, b: &mut [u8]) -> Result<FaceInfo, InspectResult> {
        let mut i = FaceInfo::default();
        let mut italic = 0;
        let e = unsafe {
            nf_describe(
                self.raw(),
                b.as_mut_ptr(),
                b.len(),
                &mut i.family_length,
                &mut i.weight,
                &mut italic,
            )
        };
        i.italic = italic != 0;
        if e == 0 {
            Ok(i)
        } else {
            Err(InspectResult::Unsupported)
        }
    }
    fn stream_failed(&self) -> bool {
        self.failed || self.face.is_some() && unsafe { nf_stream_failed(self.raw()) != 0 }
    }
}
impl Drop for Native<'_, '_> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod stream_callback_tests {
    use super::*;
    struct ReadsBeforeWrite;
    impl ReadAt for ReadsBeforeWrite {
        fn read_at(&mut self, offset: u64, output: &mut [u8]) -> usize {
            assert_eq!(offset, 7);
            assert_eq!(output, &[0; 8]);
            output.fill(0x5a);
            output.len()
        }
    }
    #[test]
    fn stream_callback_initializes_only_the_borrowed_output() {
        let mut reader = ReadsBeforeWrite;
        let mut context = StreamContext {
            reader: &mut reader,
            length: 100,
        };
        let mut buffer = [0xa5; 12];
        let n = unsafe {
            stream_read(
                (&mut context as *mut StreamContext<'_>).cast(),
                7,
                buffer.as_mut_ptr().add(2),
                8,
            )
        };
        assert_eq!(n, 8);
        assert_eq!(&buffer[..2], &[0xa5; 2]);
        assert_eq!(&buffer[2..10], &[0x5a; 8]);
        assert_eq!(&buffer[10..], &[0xa5; 2]);
    }
    struct Chunks {
        step: i32,
        fail_at: u64,
    }
    impl BookSource for Chunks {
        fn size(&self) -> u64 {
            10
        }
        fn read_at(&mut self, at: u64, output: &mut [u8]) -> i32 {
            if at >= self.fail_at {
                return -1;
            }
            let n = output
                .len()
                .min(self.step as usize)
                .min(10 - at.min(10) as usize);
            for (i, b) in output[..n].iter_mut().enumerate() {
                *b = b'0' + at as u8 + i as u8;
            }
            // A step above the size models a source over-reporting its count.
            if self.step > 10 { self.step } else { n as i32 }
        }
    }
    #[test]
    fn book_stream_preserves_one_read_and_rejects_invalid_counts() {
        let mut source = Chunks {
            step: 3,
            fail_at: u64::MAX,
        };
        let mut out = [0; 8];
        assert_eq!(BookStream::new(&mut source).read_at(1, &mut out), 3);
        assert_eq!(&out, b"123\0\0\0\0\0");
        source.fail_at = 1;
        assert_eq!(BookStream::new(&mut source).read_at(1, &mut [0; 8]), 0);
        source.fail_at = u64::MAX;
        assert_eq!(BookStream::new(&mut source).read_at(8, &mut [0; 4]), 2);
        source.step = 0;
        assert_eq!(BookStream::new(&mut source).read_at(0, &mut [0; 4]), 0);
        source.step = 11;
        assert_eq!(BookStream::new(&mut source).read_at(0, &mut [0; 4]), 0);
    }
}
