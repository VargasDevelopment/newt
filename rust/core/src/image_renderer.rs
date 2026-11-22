//! PNG path and box RowScaler from pinned ImageRenderer.cpp. JPEG is separate work.
#![forbid(unsafe_code)]
use crate::page_renderer::{BookStatus, ImageRows, SafetyError};
use crate::{
    inflate,
    pngle::{Buffers, CallbackError, Callbacks, Decoder, Error, Header, Math},
};

pub fn rgba_to_gray(p: [u8; 4]) -> u8 {
    let gray = (u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000;
    (255 - ((255 - gray) * u32::from(p[3])) / 255) as u8
}
pub struct RowScaler<'a> {
    pub src_w: u16,
    pub src_h: u16,
    pub dst_w: u16,
    pub dst_h: u16,
    row: &'a mut [u8],
    sums: &'a mut [u32],
    counts: &'a mut [u16],
    next: u32,
    pub stopped: bool,
}
impl<'a> RowScaler<'a> {
    pub fn new(
        src_w: u16,
        src_h: u16,
        dst_w: u16,
        dst_h: u16,
        row: &'a mut [u8],
        sums: &'a mut [u32],
        counts: &'a mut [u16],
    ) -> Result<Self, BookStatus> {
        if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
            return Err(BookStatus::Unsupported);
        }
        let n = usize::from(dst_w);
        if row.len() < n || sums.len() < n || counts.len() < n {
            return Err(BookStatus::OutOfMemory);
        }
        sums[..n].fill(0);
        counts[..n].fill(0);
        Ok(Self {
            src_w,
            src_h,
            dst_w,
            dst_h,
            row,
            sums,
            counts,
            next: 0,
            stopped: false,
        })
    }
    fn emit(&mut self, sink: &mut dyn ImageRows) -> Result<bool, SafetyError> {
        for x in 0..usize::from(self.dst_w) {
            self.row[x] = if self.counts[x] != 0 {
                (self.sums[x] / u32::from(self.counts[x])) as u8
            } else {
                255
            };
        }
        if !sink.on_row(
            self.next as u16,
            &self.row[..usize::from(self.dst_w)],
            self.dst_w,
        )? {
            self.stopped = true;
            return Ok(false);
        }
        self.next += 1;
        self.sums[..usize::from(self.dst_w)].fill(0);
        self.counts[..usize::from(self.dst_w)].fill(0);
        Ok(true)
    }
    pub fn feed(
        &mut self,
        y: u32,
        gray: &[u8],
        sink: &mut dyn ImageRows,
    ) -> Result<bool, SafetyError> {
        if gray.len() < usize::from(self.src_w) {
            return Err(SafetyError::RowSlice);
        }
        while self.next < u32::from(self.dst_h)
            && u64::from(y)
                >= u64::from(self.next + 1) * u64::from(self.src_h) / u64::from(self.dst_h)
        {
            if !self.emit(sink)? {
                return Ok(false);
            }
        }
        if self.next >= u32::from(self.dst_h) {
            return Ok(true);
        }
        for x in 0..usize::from(self.src_w) {
            let dx = (x as u64 * u64::from(self.dst_w) / u64::from(self.src_w)) as usize;
            // Match the original fixed-width accumulators, including defined unsigned wrap.
            self.sums[dx] = self.sums[dx].wrapping_add(u32::from(gray[x]));
            self.counts[dx] = self.counts[dx].wrapping_add(1);
        }
        if y.wrapping_add(1) == u32::from(self.src_h) {
            while self.next < u32::from(self.dst_h) {
                if !self.emit(sink)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
pub trait EntryInput {
    fn read(&mut self, dst: &mut [u8]) -> i32;
}
impl<S: crate::zip_window::source::BookSource + ?Sized> EntryInput
    for crate::zip_container::Reader<'_, S>
{
    fn read(&mut self, dst: &mut [u8]) -> i32 {
        self.read(dst)
    }
}
struct PngState<'a, 'b> {
    scaler: RowScaler<'a>,
    src: &'a mut [u8],
    current_y: u32,
    unsupported: bool,
    sink: &'b mut dyn ImageRows,
    safety: Option<SafetyError>,
}
impl Callbacks for PngState<'_, '_> {
    fn init(&mut self, h: Header) -> Result<(), CallbackError> {
        self.unsupported = h.interlace != 0;
        Ok(())
    }
    fn draw(&mut self, x: u32, y: u32, _w: u32, _h: u32, p: [u8; 4]) -> Result<(), CallbackError> {
        if self.unsupported || self.scaler.stopped {
            return Ok(());
        }
        if y != self.current_y {
            match self.scaler.feed(self.current_y, self.src, self.sink) {
                Ok(false) => return Ok(()),
                Ok(true) => {}
                Err(e) => {
                    self.safety = Some(e);
                    return Err(CallbackError);
                }
            }
            self.current_y = y;
        }
        if x < u32::from(self.scaler.src_w) {
            self.src[x as usize] = rgba_to_gray(p);
        }
        Ok(())
    }
    fn done(&mut self) -> Result<(), CallbackError> {
        Ok(())
    }
}
pub struct RenderBuffers<'a> {
    pub src: &'a mut [u8],
    pub read: &'a mut [u8],
    pub dst: &'a mut [u8],
    pub sums: &'a mut [u32],
    pub counts: &'a mut [u16],
}
/// Matches the original EOF/partial-feed behavior, including its permissive truncation.
/// Low-level clients can separately require Decoder::eof(); the wrapper does not.
pub fn render_png(
    input: &mut dyn EntryInput,
    src_w: u16,
    src_h: u16,
    dst_w: u16,
    dst_h: u16,
    decoder: &mut Decoder,
    inflate: &mut inflate::State,
    png: &mut Buffers<'_>,
    work: RenderBuffers<'_>,
    math: &dyn Math,
    sink: &mut dyn ImageRows,
) -> Result<BookStatus, SafetyError> {
    if work.src.len() < usize::from(src_w) || work.read.len() < 4096 {
        return Ok(BookStatus::OutOfMemory);
    }
    let scaler = match RowScaler::new(src_w, src_h, dst_w, dst_h, work.dst, work.sums, work.counts)
    {
        Ok(v) => v,
        Err(s) => return Ok(s),
    };
    work.src[..usize::from(src_w)].fill(255);
    decoder.reset(inflate);
    let mut state = PngState {
        scaler,
        src: &mut work.src[..usize::from(src_w)],
        current_y: 0,
        unsupported: false,
        sink,
        safety: None,
    };
    let mut status = BookStatus::Ok;
    loop {
        let n = input.read(&mut work.read[..4096]);
        if n < 0 {
            status = BookStatus::IoError;
            break;
        }
        if n == 0 {
            break;
        }
        if n > 4096 {
            return Err(SafetyError::RowSlice);
        }
        if let Err(e) = decoder.feed(&work.read[..n as usize], png, inflate, math, &mut state) {
            status = if matches!(e, Error::Capacity { .. }) {
                BookStatus::OutOfMemory
            } else {
                BookStatus::ParseError
            };
            break;
        }
        if state.unsupported {
            status = BookStatus::Unsupported;
            break;
        }
        if state.scaler.stopped {
            break;
        }
    }
    if let Some(e) = state.safety {
        return Err(e);
    }
    if status == BookStatus::Ok && !state.scaler.stopped {
        state.scaler.feed(state.current_y, state.src, state.sink)?;
    }
    Ok(status)
}
/// Directory lookup is borrowed; the accepted Catalog can supply this without copying data.
pub trait EntryLookup {
    fn find_entry(&self, href: &[u8]) -> Option<crate::zip_container::Entry>;
}
/// A resolved accepted Catalog entry can release the catalog borrow before borrowing its source.
pub struct ResolvedEntry<'a> {
    pub href: &'a [u8],
    pub entry: crate::zip_container::Entry,
}
impl EntryLookup for ResolvedEntry<'_> {
    fn find_entry(&self, href: &[u8]) -> Option<crate::zip_container::Entry> {
        (href == self.href).then_some(self.entry)
    }
}
/// Full PNG lookup/probe/reopen path. A separate JPEG decoder can wrap this dispatcher.
pub struct ZipPngDecoder<'a, S: crate::zip_window::source::BookSource + ?Sized> {
    pub source: &'a mut S,
    pub catalog: &'a dyn EntryLookup,
    pub zip_state: &'a mut inflate::State,
    pub zip_dictionary: &'a mut [u8; inflate::DICTIONARY_SIZE],
    pub zip_input: &'a mut [u8; crate::zip_container::INPUT_SIZE],
    pub probe_skip: &'a mut [u8],
    pub decoder: &'a mut Decoder,
    pub png_state: &'a mut inflate::State,
    pub png: Buffers<'a>,
    pub work: RenderBuffers<'a>,
    pub math: &'a dyn Math,
}
fn status(s: crate::zip_container::Status) -> BookStatus {
    use crate::zip_container::Status as S;
    match s {
        S::Ok => BookStatus::Ok,
        S::IoError => BookStatus::IoError,
        S::NotZip => BookStatus::NotZip,
        S::NotEpub => BookStatus::NotEpub,
        S::Encrypted => BookStatus::Encrypted,
        S::Truncated => BookStatus::Truncated,
        S::Unsupported => BookStatus::Unsupported,
        S::OutOfMemory => BookStatus::OutOfMemory,
        S::ParseError => BookStatus::ParseError,
        S::NotFound => BookStatus::NotFound,
        S::Stale => BookStatus::Stale,
    }
}
impl<S: crate::zip_window::source::BookSource + ?Sized> crate::page_renderer::ImageDecoder
    for ZipPngDecoder<'_, S>
{
    fn render(
        &mut self,
        image: &crate::page_renderer::PageImage<'_>,
        _scratch: &mut crate::page_renderer::ByteArena<'_>,
        rows: &mut dyn ImageRows,
    ) -> Result<BookStatus, SafetyError> {
        if image.width == 0 || image.height == 0 {
            return Ok(BookStatus::Unsupported);
        }
        let entry = match self.catalog.find_entry(image.href) {
            Some(e) => e,
            None => return Ok(BookStatus::NotFound),
        };
        let mut info = crate::image_probe::ImageInfo::default();
        let probe = crate::image_probe::probe(
            self.source,
            &entry,
            Some(crate::zip_container::InflateScratch {
                state: self.zip_state,
                dictionary: self.zip_dictionary,
                input: self.zip_input,
            }),
            self.probe_skip,
            &mut info,
        );
        if probe != crate::zip_container::Status::Ok {
            return Ok(status(probe));
        }
        if info.kind != crate::image_probe::Kind::Png {
            return Ok(BookStatus::Unsupported);
        }
        let mut reader = crate::zip_container::Reader::new(
            self.source,
            Some(crate::zip_container::InflateScratch {
                state: self.zip_state,
                dictionary: self.zip_dictionary,
                input: self.zip_input,
            }),
        );
        let opened = reader.open(&entry);
        if opened != crate::zip_container::Status::Ok {
            return Ok(status(opened));
        }
        render_png(
            &mut reader,
            info.width,
            info.height,
            image.width,
            image.height,
            self.decoder,
            self.png_state,
            &mut self.png,
            RenderBuffers {
                src: self.work.src,
                read: self.work.read,
                dst: self.work.dst,
                sums: self.work.sums,
                counts: self.work.counts,
            },
            self.math,
            rows,
        )
    }
}
