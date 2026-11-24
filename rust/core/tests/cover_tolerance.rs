//! #316: the cover decoder accepts what the frozen C++ (TJpgDec) drew, and old
//! Rust no-art records are read once more. Payload CRCs below are the frozen
//! C++ cover session's rasters for these exact bytes (stored in an EPUB).
use newt_core::cover::{self, BareRecord, NoCoverReason, Record, png, jpeg::{Layout, Status, validate, dither_row, placement}};
use newt_core::jpeg::{Error, Input, Workspace, progressive, renderer::{RenderError, Rows, render}};
use newt_core::page_renderer::BookStatus;
use newt_core::page_renderer::{ImageRows, SafetyError};
use newt_core::zip_window::source::BookSource;

/// Authored grayscale baseline JPEG: quantizers 1, DC category k coded as 4-bit k,
/// AC holds only EOB (`00`). Each 8x8 block is flat at its DC value. A
/// progressive one (SOF2) holds only its DC first scan, at point transform `al`.
/// With a Y `sampling`, it is YCbCr with flat chroma, interleaved in MCUs.
#[derive(Clone)]
struct Jpeg {
    width: u16,
    height: u16,
    dc: Vec<i32>,
    dc_counts: [u8; 16],
    dc_symbols: Vec<u8>,
    sos_tail: Vec<u8>,
    progressive: Option<u8>,
    sampling: Option<u8>,
}
impl Default for Jpeg {
    fn default() -> Self {
        let mut dc_counts = [0; 16];
        dc_counts[3] = 12;
        Self { width: 64, height: 48, dc: vec![0, 64, 128, -64, 192, -128, 256, -192, 512],
               dc_counts, dc_symbols: (0..12).collect(), sos_tail: vec![0, 63, 0], progressive: None,
               sampling: None }
    }
}
fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
    out.extend([0xff, marker]);
    out.extend((body.len() as u16 + 2).to_be_bytes());
    out.extend(body);
}
impl Jpeg {
    fn bytes(&self) -> Vec<u8> {
        let mut out = vec![0xff, 0xd8];
        segment(&mut out, 0xdb, &[[0].as_slice(), &[1; 64]].concat());
        let [h0, h1] = self.height.to_be_bytes();
        let [w0, w1] = self.width.to_be_bytes();
        let (frame, scan) = match self.sampling {
            None => (vec![1, 1, 0x11, 0], vec![1, 1, 0x00]),
            Some(y) => (vec![3, 1, y, 0, 2, 0x11, 0, 3, 0x11, 0], vec![3, 1, 0x00, 2, 0x11, 3, 0x11]),
        };
        segment(&mut out, if self.progressive.is_some() { 0xc2 } else { 0xc0 }, &[[8, h0, h1, w0, w1].as_slice(), &frame].concat());
        let mut ac_counts = [0; 16];
        ac_counts[1] = 1;
        // Chroma (tables 1) codes the same way as Y (tables 0).
        for id in if self.sampling.is_some() { 0..2 } else { 0..1 } {
            segment(&mut out, 0xc4, &[[id].as_slice(), &self.dc_counts, &self.dc_symbols, &[0x10 | id], &ac_counts, &[0]].concat());
        }
        let tail = self.progressive.map_or(self.sos_tail.clone(), |al| vec![0, 0, al]);
        segment(&mut out, 0xda, &[scan, tail].concat());
        let mut bits: Vec<bool> = Vec::new();
        let mut put = |value: u32, length: u32| bits.extend((0..length).rev().map(|i| value >> i & 1 != 0));
        let (bw, bh) = (usize::from(self.width.div_ceil(8)), usize::from(self.height.div_ceil(8)));
        // Each coded block as (component, DC), in scan order; MCU padding past the image is 0.
        let y = |x: usize, y: usize| if x < bw && y < bh { self.dc[(y * bw + x) % self.dc.len()] } else { 0 };
        let mut units = Vec::new();
        match self.sampling {
            None => units.extend((0..bw * bh).map(|b| (0, y(b % bw, b / bw)))),
            Some(sampling) => {
                let (h, v) = (usize::from(sampling >> 4), usize::from(sampling & 15));
                for row in 0..bh.div_ceil(v) {
                    for column in 0..bw.div_ceil(h) {
                        units.extend((0..h * v).map(|b| (0, y(column * h + b % h, row * v + b / h))));
                        units.extend([(1, 0), (2, 0)]);
                    }
                }
            }
        }
        let mut previous = [0; 3];
        for (component, dc) in units {
            let target = dc >> self.progressive.unwrap_or(0);
            let diff = target - previous[component];
            previous[component] = target;
            let category = 32 - diff.unsigned_abs().leading_zeros();
            put(category, 4);
            put((if diff < 0 { diff + (1 << category) - 1 } else { diff }) as u32, category);
            if self.progressive.is_none() { put(0, 2); }
        }
        bits.resize(bits.len().div_ceil(8) * 8, true);
        for byte in bits.chunks(8) {
            let byte = byte.iter().fold(0u8, |acc, &bit| acc << 1 | u8::from(bit));
            out.push(byte);
            if byte == 0xff { out.push(0); }
        }
        out.extend([0xff, 0xd9]);
        out
    }
}

struct Bytes<'a>(&'a [u8]);
impl Input for Bytes<'_> {
    fn read(&mut self, dst: &mut [u8]) -> Result<usize, Error> {
        let n = dst.len().min(self.0.len());
        dst[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}
impl BookSource for Bytes<'_> {
    fn size(&self) -> u64 { self.0.len() as u64 }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Some(rest) = self.0.get(at as usize..) else { return -1 };
        let n = out.len().min(rest.len());
        out[..n].copy_from_slice(&rest[..n]);
        n as i32
    }
}
/// The centered, dithered 144x188 cover raster, as firmware cover_decode.rs builds it.
struct Raster { px: [u8; cover::PIXELS], width: u16, left: usize, top: usize, rows: u16 }
impl ImageRows for Raster {
    fn on_row(&mut self, y: u16, gray: &[u8], width: u16) -> Result<bool, SafetyError> {
        assert_eq!((y, width), (self.rows, self.width));
        let mut bits = [0; 18];
        let y = usize::from(y) + self.top;
        dither_row(&gray[..usize::from(width)], y, &mut bits);
        for x in (0..usize::from(width)).filter(|x| bits[x / 8] & (0x80 >> (x % 8)) != 0) {
            let t = x + self.left;
            self.px[y * 18 + t / 8] |= 0x80 >> (t % 8);
        }
        self.rows += 1;
        Ok(true)
    }
}
/// Grey rows as the firmware's cover decode receives them, before dithering.
#[derive(Default)]
struct Grey(Vec<Vec<u8>>);
impl ImageRows for Grey {
    fn on_row(&mut self, _y: u16, gray: &[u8], width: u16) -> Result<bool, SafetyError> {
        self.0.push(gray[..usize::from(width)].to_vec());
        Ok(true)
    }
}
/// Renders `image` at its cover placement into `sink`; returns that placement.
fn render_cover(image: &[u8], sink: &mut dyn ImageRows) -> Result<(u16, u16), RenderError> {
    let mut layout = Layout::default();
    assert!(validate(&mut Bytes(image), &mut layout) == Status::Ready);
    let (w, h) = placement(layout.width, layout.height);
    let (mut input, mut segment, mut quant, mut huffman) = ([0; 512], [0; 512], [0; 256], [0; 1088]);
    let (mut block, mut mcu, mut pixels) = ([0; 64], [0; 384], [0; 256]);
    let workspace = Workspace { input: &mut input, segment: &mut segment, quant: &mut quant, huffman: &mut huffman,
                                block: &mut block, mcu: &mut mcu, pixels: &mut pixels };
    let (mut band, mut row, mut sums, mut counts) = (vec![0; 16 * usize::from(layout.width)], vec![0; usize::from(w)],
                                                     vec![0; usize::from(w)], vec![0; usize::from(w)]);
    let rows = Rows { band: &mut band, row: &mut row, sums: &mut sums, counts: &mut counts };
    render(&mut Bytes(image), workspace, rows, w, h, sink)?;
    Ok((w, h))
}
/// The cover payload CRC, or the renderer's refusal (a ParseError becomes a no-art record).
fn cover_crc(image: &[u8]) -> Result<u32, RenderError> {
    let mut layout = Layout::default();
    assert!(validate(&mut Bytes(image), &mut layout) == Status::Ready);
    let (w, h) = placement(layout.width, layout.height);
    let mut raster = Raster { px: [0; cover::PIXELS], width: w, left: usize::from(144 - w) / 2,
                              top: usize::from(188 - h) / 2, rows: 0 };
    render_cover(image, &mut raster)?;
    assert_eq!(raster.rows, h);
    Ok(cover::crc(u32::MAX, &raster.px) ^ u32::MAX)
}

const BASE: u32 = 0x6254_f99c;

#[test]
fn spectral_selection_bytes_are_ignored() {
    assert_eq!(cover_crc(&Jpeg::default().bytes()).unwrap(), BASE);
    for tail in [[0, 63, 1], [1, 5, 0], [0, 0, 0xff]] {
        let image = Jpeg { sos_tail: tail.to_vec(), ..Jpeg::default() };
        assert_eq!(cover_crc(&image.bytes()).unwrap(), BASE, "{tail:?}");
    }
}

#[test]
fn sos_length_needs_only_the_bytes_read() {
    for tail in [vec![], vec![0, 63, 0, 0x55]] {
        let image = Jpeg { sos_tail: tail.clone(), ..Jpeg::default() };
        assert_eq!(cover_crc(&image.bytes()).unwrap(), BASE, "{tail:?}");
    }
    // One component's table byte missing: native reads stale bytes; Rust refuses.
    let mut short = Jpeg { sos_tail: vec![], ..Jpeg::default() }.bytes();
    let sos = short.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
    short.splice(sos + 2..sos + 7, [0, 3, 1]);
    assert!(cover_crc(&short).is_err());
}

#[test]
fn over_subscribed_huffman_counts_decode() {
    // 17 four-bit DC codes where 16 fit: the extra code can never match.
    let mut image = Jpeg::default();
    image.dc_counts[3] = 17;
    image.dc_symbols.extend([0; 5]);
    assert_eq!(cover_crc(&image.bytes()).unwrap(), BASE);
}

#[test]
fn dc_predictor_wraps_like_tjpgdec() {
    // Steps of 2000 carry the predictor past i16::MAX; TJpgDec narrows only the stored predictor.
    let image = Jpeg { width: 192, height: 8, dc: (0..24).map(|i| 2000 * i).collect(), ..Jpeg::default() };
    assert_eq!(cover_crc(&image.bytes()).unwrap(), 0xc2c8_2d8c);
}

#[test]
fn dc_predictor_wrap_keeps_the_full_value_in_the_pixels() {
    // The last block's DC, 34017, is past i16::MAX and not a multiple of 8, so the
    // truncating /8 tells the full value (grey 28) from one narrowed first (29).
    // Multiples of 8, as above, give the same grey either way.
    let mut dc: Vec<i32> = (0..17).map(|i| 2000 * i).collect();
    dc.push(34017);
    let image = Jpeg { width: 144, height: 8, dc, ..Jpeg::default() };
    let mut grey = Grey::default();
    assert_eq!(render_cover(&image.bytes(), &mut grey).unwrap(), (144, 8));
    assert_eq!(grey.0.len(), 8);
    for row in &grey.0 {
        assert_eq!(row[..8], [128; 8]);
        assert_eq!(row[136..], [28; 8]);
    }
}

#[test]
fn formats_both_builds_reject_stay_unsupported() {
    let base = Jpeg::default().bytes();
    let sof = base.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    let mut arithmetic = base.clone();
    arithmetic[sof + 1] = 0xc9;
    let mut cmyk = base.clone();
    cmyk[sof + 9] = 4;
    for image in [arithmetic.as_slice(), &cmyk] {
        assert!(validate(&mut Bytes(image), &mut Layout::default()) == Status::Unsupported);
    }
}

/// A grayscale 8-bit PNG's signature, IHDR and IEND: all `png::validate` reads.
fn png_header(width: u32, height: u32, interlace: u8) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    out.extend(width.to_be_bytes());
    out.extend(height.to_be_bytes());
    // Depth, color, compression, filter, interlace, then an unchecked CRC.
    out.extend([8, 0, 0, 0, interlace, 0, 0, 0, 0]);
    out.extend([0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]);
    out
}

#[test]
fn a_png_row_past_the_cap_or_interlaced_is_unsupported() {
    // Grey 8-bit: the row is the width, and the ring the width plus two.
    let fits = (png::ROW_BYTES - 2) / 2;
    let mut layout = Layout::default();
    assert!(png::validate(&mut Bytes(&png_header(fits as u32, 8, 0)), &mut layout) == Status::Ready);
    assert_eq!((usize::from(layout.width), usize::from(layout.ring)), (fits, fits + 2));
    for image in [png_header(fits as u32 + 1, 8, 0), png_header(64, 8, 1)] {
        assert!(png::validate(&mut Bytes(&image), &mut Layout::default()) == Status::Unsupported);
    }
}

#[test]
fn a_png_whose_slot_pixel_counts_past_u16_is_unsupported() {
    // 1024 wide takes a 4x188 slot up to 47,940 rows: 256 x 255 samples per
    // slot pixel. One row more needs 256 x 256, past the scaler's u16 counts.
    let mut layout = Layout::default();
    assert!(png::validate(&mut Bytes(&png_header(1024, 47_940, 0)), &mut layout) == Status::Ready);
    for height in [47_941, 65_535] {
        assert!(png::validate(&mut Bytes(&png_header(1024, height, 0)), &mut layout) == Status::Unsupported, "{height}");
    }
}

/// `body` as a PNG chunk, with its CRC.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend((body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(body);
    let crc = cover::crc(u32::MAX, &out[start..]) ^ u32::MAX;
    out.extend(crc.to_be_bytes());
}
/// `raw` as a zlib stream of stored blocks; `end` false leaves the last block
/// open and the checksum out.
fn zlib(raw: &[u8], end: bool) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if raw.is_empty() { vec![&[]] } else { raw.chunks(0xffff).collect() };
    for (i, block) in blocks.iter().enumerate() {
        out.push(u8::from(end && i + 1 == blocks.len()));
        let len = block.len() as u16;
        out.extend(len.to_le_bytes());
        out.extend((!len).to_le_bytes());
        out.extend(*block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in raw { a = (a + u32::from(byte)) % 65_521; b = (b + a) % 65_521; }
    if end { out.extend((b << 16 | a).to_be_bytes()); }
    out
}
/// A grey 8-bit PNG of `width` x `height` whose one IDAT is `idat`.
fn png_image(width: u32, height: u32, idat: &[u8]) -> Vec<u8> {
    let mut out = png::SIGNATURE.to_vec();
    chunk(&mut out, b"IHDR", &[width.to_be_bytes().as_slice(), &height.to_be_bytes(), &[8, 0, 0, 0, 0]].concat());
    chunk(&mut out, b"IDAT", idat);
    chunk(&mut out, b"IEND", &[]);
    out
}
/// A PNG at its cover placement, as grey rows, as firmware cover_decode.rs draws it.
fn render_png(image: &[u8]) -> Result<Grey, BookStatus> {
    let mut layout = Layout::default();
    assert!(png::validate(&mut Bytes(image), &mut layout) == Status::Ready);
    let (w, h) = placement(layout.width, layout.height);
    let mut inflate = Box::new(newt_core::inflate::State::new(newt_core::inflate::BitBuffer::Bits32));
    let (mut read, mut dictionary) = (vec![0; png::READ], vec![0; newt_core::inflate::DICTIONARY_SIZE]);
    let (mut palette, mut transparency) = (vec![0; png::PALETTE], vec![0; png::TRANSPARENCY]);
    let (mut ring, mut src) = (vec![0; usize::from(layout.ring)], vec![0; usize::from(layout.width)]);
    let (mut dst, mut sums, mut counts) = (vec![0; usize::from(w)], vec![0; usize::from(w)], vec![0; usize::from(w)]);
    let buffers = png::Buffers { read: &mut read, inflate: &mut inflate, dictionary: &mut dictionary, palette: &mut palette,
                                 transparency: &mut transparency, ring: &mut ring, src: &mut src, dst: &mut dst,
                                 sums: &mut sums, counts: &mut counts };
    let mut grey = Grey::default();
    match png::render(&mut Bytes(image), buffers, (layout.width, layout.height), (w, h), &mut grey) {
        Ok(()) => Ok(grey),
        Err(RenderError::Status(status)) => Err(status),
        Err(RenderError::Safety(error)) => panic!("{error:?}"),
    }
}

#[test]
fn a_png_is_drawn_only_when_every_pixel_arrives() {
    // Rows of grey 128, each led by filter 0. IEND follows in every case.
    let rows = |width: usize, height: usize| [[0].as_slice(), &vec![128; width]].concat().repeat(height);
    let whole = render_png(&png_image(16, 4, &zlib(&rows(16, 4), true))).unwrap();
    assert_eq!(whole.0, vec![vec![128; 16]; 4]);
    let short = [rows(16, 3), rows(10, 1)].concat();
    // 255x128 fills pngle's 32 KiB window exactly, so every pixel is drawn
    // though the stream never ends.
    for (name, image) in [("no pixels", png_image(288, 1, &zlib(&[], true))),
                          ("last row short", png_image(16, 4, &zlib(&short, true))),
                          ("zlib unfinished", png_image(255, 128, &zlib(&rows(255, 128), false)))] {
        assert_eq!(render_png(&image).err(), Some(BookStatus::ParseError), "{name}");
    }
}

/// A progressive file's first scan at its cover placement, sent into `sink`.
fn render_progressive_into(image: &[u8], sink: &mut dyn ImageRows) -> Result<(), BookStatus> {
    let mut layout = Layout::default();
    assert!(validate(&mut Bytes(image), &mut layout) == Status::Ready && layout.progressive);
    let (w, h) = placement(layout.width, layout.height);
    let (mut input, mut tables) = ([0; 512], [progressive::Huffman::default(); 4]);
    let (mut grid, mut previous, mut current, mut output) = ([0; 512], [0; 144], [0; 144], [0; 144]);
    let buffers = progressive::Buffers { input: &mut input, tables: &mut tables, grid: &mut grid,
                                         previous: &mut previous, current: &mut current, output: &mut output };
    match progressive::render(&mut Bytes(image), buffers, w, h, sink) {
        Ok(()) => Ok(()),
        Err(RenderError::Status(status)) => Err(status),
        Err(RenderError::Safety(error)) => panic!("{error:?}"),
    }
}
/// A progressive file's first scan at its cover placement, as grey rows.
fn render_progressive(image: &[u8]) -> Result<Grey, BookStatus> {
    let mut grey = Grey::default();
    render_progressive_into(image, &mut grey)?;
    Ok(grey)
}

#[test]
fn a_progressive_first_scan_draws_its_dc_greys() {
    // 8x2 blocks; the point transform halves each coded value. Grey = 128 + DC/8.
    let dc: Vec<i32> = (0..16).map(|i| (i * 16 - 128) * 8).collect();
    let image = Jpeg { width: 64, height: 16, dc, progressive: Some(1), ..Jpeg::default() };
    let grey = render_progressive(&image.bytes()).unwrap();
    assert_eq!(grey.0.len(), 16);
    // Corners hold their blocks' greys exactly; between, rows blend bilinearly.
    assert_eq!((grey.0[0][0], grey.0[0][63], grey.0[15][0], grey.0[15][63]), (0, 112, 128, 240));
    assert!(grey.0.iter().all(|row| row.windows(2).all(|pair| pair[0] <= pair[1])));
}

#[test]
fn a_progressive_file_without_a_dc_first_scan_is_unsupported() {
    // A first scan of AC bands (Ss 1, Se 63) holds no DC greys to draw.
    let mut image = Jpeg { progressive: Some(0), ..Jpeg::default() }.bytes();
    let sos = image.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
    image[sos + 7..sos + 9].copy_from_slice(&[1, 63]);
    assert_eq!(render_progressive(&image).err(), Some(BookStatus::Unsupported));
}

#[test]
fn a_progressive_first_scan_without_luminance_is_unsupported() {
    // A YCbCr file whose first scan is Cb and Cr DC only: no Y greys to draw.
    let mut image = Jpeg { progressive: Some(0), sampling: Some(0x11), ..Jpeg::default() }.bytes();
    let sos = image.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
    image.splice(sos + 2..sos + 7, [0, 10, 2]);
    assert_eq!(render_progressive(&image).err(), Some(BookStatus::Unsupported));
}

#[test]
fn y_sampling_12_and_41_draw_the_grey_frame() {
    // Chroma never reaches the greys, so a YCbCr frame draws its Y blocks as a
    // grayscale one does. These sizes pad MCUs and take baseline scales 0, 1 and 3.
    let dc: Vec<i32> = (0..23).map(|i| (i * 11 - 128) * 8).collect();
    let baseline = |image: &Jpeg| { let mut grey = Grey::default(); render_cover(&image.bytes(), &mut grey).unwrap(); grey.0 };
    for (width, height) in [(72, 40), (320, 72), (1200, 200)] {
        let grey = Jpeg { width, height, dc: dc.clone(), ..Jpeg::default() };
        for sampling in [0x12, 0x41] {
            let image = Jpeg { sampling: Some(sampling), ..grey.clone() };
            assert_eq!(baseline(&image), baseline(&grey), "{sampling:#x} at {width}x{height}");
            if width == 72 {
                let (image, grey) = (Jpeg { progressive: Some(1), ..image }, Jpeg { progressive: Some(1), ..grey.clone() });
                assert_eq!(render_progressive(&image.bytes()).unwrap().0, render_progressive(&grey.bytes()).unwrap().0,
                           "progressive {sampling:#x}");
            }
        }
    }
}

#[test]
fn old_no_art_records_retry_once() {
    // Legacy unreasoned negatives (CJB3..=CJB7) retry once so Books learns a reason.
    for route in [*b"CJB3", *b"CJB4", *b"CJB5", *b"CJB6", *b"CJB7"].map(u32::from_le_bytes) {
        assert_eq!(cover::record(None, route), Record::Generate);
        assert_eq!(cover::bare_record(route), BareRecord::Retry);
    }
    // Current versioned routes (`CJ` + version + reason) settle with their exact copy.
    let cases = [
        (NoCoverReason::Missing, *b"CJCM", b"No cover in this book".as_slice()),
        (NoCoverReason::Unsupported, *b"CJCU", b"Cover format not supported".as_slice()),
        (NoCoverReason::Incomplete, *b"CJCI", b"Cover file is incomplete".as_slice()),
        (NoCoverReason::Invalid, *b"CJCX", b"Cover file is damaged".as_slice()),
    ];
    for (reason, tag, note) in cases {
        let route = cover::no_art_route(reason);
        assert_eq!(route, u32::from_le_bytes(tag));
        assert_eq!(reason.note(), note);
        let header = cover::header(1234, 5678, route);
        let written = u32::from_le_bytes(header[24..].try_into().unwrap());
        assert_eq!(cover::record(None, written), Record::Current);
        assert_eq!(cover::bare_record(written), BareRecord::Current(reason));
        // Bumping the version byte retries every prior reasoned record without a static list.
        assert_eq!(cover::bare_record_at(written, cover::NO_ART_VERSION + 1), BareRecord::Retry);
        // A route value is never a picture, and art whose CRC equals a route word stays valid.
        assert_eq!(cover::record(Some(BASE), route), Record::Generate);
        assert_eq!(cover::record(Some(route), route), Record::Current);
    }
    // Unknown reason bytes, future versions, and non-CJ words are rejected as Other.
    for bad in [*b"CJCZ", *b"CJDM", *b"CJB8", *b"XXXX"].map(u32::from_le_bytes) {
        assert_eq!(cover::bare_record(bad), BareRecord::Other);
        assert_eq!(cover::record(None, bad), Record::Generate);
    }
    assert_eq!(cover::record(Some(BASE), BASE), Record::Current);
    for route in [*b"CJB3", *b"CJB4", *b"CJB5", *b"CJB6", *b"CJB7"].map(u32::from_le_bytes) {
        assert_eq!(cover::record(Some(route), route), Record::Current);
    }
    assert_eq!(cover::record(None, BASE), Record::Generate);
}

/// Authored 3-component JPEG with flat 8x8 RGB (or YCbCr) blocks, optional
/// APP14 segment, configurable component IDs/tables/order, baseline or progressive.
#[derive(Clone)]
struct ColorJpeg {
    width: u16,
    height: u16,
    blocks: Vec<[u8; 3]>,
    app14: Option<Vec<u8>>,
    cids: [u8; 3],
    scan_order: [usize; 3],
    scan_len: usize,
    sampling: u8,
    shared_table: bool,
    quants: [u8; 3],
    progressive: Option<u8>,
}
impl Default for ColorJpeg {
    fn default() -> Self {
        Self {
            width: 64, height: 16,
            blocks: vec![
                [0, 255, 0], [0, 0, 255], [255, 0, 0], [0, 200, 200],
                [40, 220, 100], [255, 255, 255], [0, 0, 0], [16, 180, 240],
            ],
            app14: Some(adobe_app14(0)),
            cids: *b"RGB",
            scan_order: [0, 1, 2],
            scan_len: 3,
            sampling: 0x11,
            shared_table: true,
            quants: [1, 1, 1],
            progressive: None,
        }
    }
}
fn adobe_app14(transform: u8) -> Vec<u8> {
    let mut body = b"Adobe".to_vec();
    body.extend([0, 100, 0, 0, 0, 0, transform]);
    body
}
fn expected_gray([r, g, b]: [u8; 3]) -> u8 {
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8
}
impl ColorJpeg {
    fn bytes(&self) -> Vec<u8> {
        let mut out = vec![0xff, 0xd8];
        if let Some(app14) = &self.app14 {
            segment(&mut out, 0xee, app14);
        }
        for id in 0..3u8 {
            segment(&mut out, 0xdb, &[[id].as_slice(), &[self.quants[usize::from(id)]; 64]].concat());
        }
        let [h0, h1] = self.height.to_be_bytes();
        let [w0, w1] = self.width.to_be_bytes();
        let frame = vec![
            8, h0, h1, w0, w1, 3,
            self.cids[0], self.sampling, 0,
            self.cids[1], 0x11, 1,
            self.cids[2], 0x11, 2,
        ];
        segment(&mut out, if self.progressive.is_some() { 0xc2 } else { 0xc0 }, &frame);
        let (mut dc_counts, mut ac_counts) = ([0u8; 16], [0u8; 16]);
        dc_counts[3] = 12;
        ac_counts[1] = 1;
        let dc_symbols: Vec<u8> = (0..12).collect();
        for id in if self.shared_table { 0..1 } else { 0..2 } {
            segment(&mut out, 0xc4, &[[id].as_slice(), &dc_counts, &dc_symbols, &[0x10 | id], &ac_counts, &[0]].concat());
        }
        let mut scan = vec![self.scan_len as u8];
        for &ci in &self.scan_order[..self.scan_len] {
            let tbl = if self.shared_table || ci == 0 { 0x00 } else { 0x11 };
            scan.extend([self.cids[ci], tbl]);
        }
        scan.extend(self.progressive.map_or([0, 63, 0], |al| [0, 0, al]));
        segment(&mut out, 0xda, &scan);
        let (bw, bh) = (usize::from(self.width.div_ceil(8)), usize::from(self.height.div_ceil(8)));
        let (h, v) = (usize::from(self.sampling >> 4), usize::from(self.sampling & 15));
        let dc_of = |bx: usize, by: usize, ci: usize| -> i32 {
            let sample = if bx < bw && by < bh {
                self.blocks[(by * bw + bx) % self.blocks.len()][ci]
            } else { 128 };
            (i32::from(sample) - 128) * 8 / i32::from(self.quants[ci])
        };
        let mut units = Vec::new();
        if self.scan_len == 1 {
            let ci = self.scan_order[0];
            for by in 0..bh { for bx in 0..bw { units.push((ci, dc_of(bx, by, ci))); } }
        } else {
            for row in 0..bh.div_ceil(v) {
                for col in 0..bw.div_ceil(h) {
                    for &ci in &self.scan_order[..self.scan_len] {
                        if ci == 0 {
                            for dy in 0..v { for dx in 0..h { units.push((0, dc_of(col * h + dx, row * v + dy, 0))); } }
                        } else {
                            units.push((ci, dc_of(col * h, row * v, ci)));
                        }
                    }
                }
            }
        }
        let mut bits = Vec::new();
        let mut put = |value: u32, length: u32| bits.extend((0..length).rev().map(|i| value >> i & 1 != 0));
        let mut previous = [0i32; 3];
        for (ci, dc) in units {
            let target = dc >> self.progressive.unwrap_or(0);
            let diff = target - previous[ci];
            previous[ci] = target;
            let category = 32 - diff.unsigned_abs().leading_zeros();
            put(category, 4);
            put((if diff < 0 { diff + (1 << category) - 1 } else { diff }) as u32, category);
            if self.progressive.is_none() { put(0, 2); }
        }
        bits.resize(bits.len().div_ceil(8) * 8, true);
        for byte in bits.chunks(8) {
            let byte = byte.iter().fold(0u8, |acc, &bit| acc << 1 | u8::from(bit));
            out.push(byte);
            if byte == 0xff { out.push(0); }
        }
        out.extend([0xff, 0xd9]);
        out
    }
}

#[test]
fn adobe_rgb_baseline_draws_true_luminance_and_ycbcr_stays_unchanged() {
    let image = ColorJpeg::default();
    let mut rgb = Grey::default();
    assert_eq!(render_cover(&image.bytes(), &mut rgb).unwrap(), (64, 16));
    for (index, &pixel) in image.blocks.iter().enumerate() {
        let got = rgb.0[0][index * 8 + 4];
        let want = expected_gray(pixel);
        assert!(got.abs_diff(want) <= 1, "block {index} ({pixel:?}): got {got}, want {want}");
    }
    // Zero-red pure green must be mid-bright (~149), never black (0).
    assert!(rgb.0[0][4] >= 148 && rgb.0[0][4] <= 150);

    // Separate Huffman tables, distinct quantizers, and reordered SOS [B, G, R] give the same greys.
    let reordered = ColorJpeg { shared_table: false, scan_order: [2, 1, 0], quants: [1, 2, 2], ..image.clone() };
    let mut other = Grey::default();
    assert_eq!(render_cover(&reordered.bytes(), &mut other).unwrap(), (64, 16));
    for (index, &pixel) in image.blocks.iter().enumerate() {
        assert!(other.0[0][index * 8 + 4].abs_diff(expected_gray(pixel)) <= 2, "reordered block {index}");
    }

    // Subsampled R (2x2) with 1x1 G and B upsamples G/B across the 16x16 MCU.
    let sub = ColorJpeg { sampling: 0x22, blocks: vec![[0, 200, 100]], ..image.clone() };
    let mut sub_grey = Grey::default();
    assert_eq!(render_cover(&sub.bytes(), &mut sub_grey).unwrap(), (64, 16));
    assert!(sub_grey.0.iter().flatten().all(|&g| g.abs_diff(expected_gray([0, 200, 100])) <= 1));

    // Scale 3 (1/8 DC-only shortcut) on a wide RGB cover draws exact luminance too.
    let wide = ColorJpeg { width: 1200, height: 160, blocks: vec![[0, 200, 200]], ..image.clone() };
    let mut scaled = Grey::default();
    let (sw, sh) = render_cover(&wide.bytes(), &mut scaled).unwrap();
    assert!((sw, sh) == (144, 19) && scaled.0.iter().flatten().all(|&g| g.abs_diff(expected_gray([0, 200, 200])) <= 1));

    // YCbCr (APP14 transform 1, no APP14, or non-Adobe APP14) stays unchanged: component 0 only.
    for app14 in [Some(adobe_app14(1)), None, Some(b"Other\0\0\0\0\0\0\0".to_vec()), Some(b"Adobe".to_vec())] {
        let ycbcr = ColorJpeg { app14, shared_table: false, ..image.clone() };
        let mut grey = Grey::default();
        render_cover(&ycbcr.bytes(), &mut grey).unwrap();
        for (index, &pixel) in image.blocks.iter().enumerate() {
            assert_eq!(grey.0[0][index * 8 + 4], pixel[0], "YCbCr block {index}");
        }
    }
    // 1-component grayscale ignores APP14 transform 0.
    let mut gray_only = Jpeg::default().bytes();
    let mut app14 = Vec::new();
    segment(&mut app14, 0xee, &adobe_app14(0));
    gray_only.splice(2..2, app14);
    assert_eq!(cover_crc(&gray_only).unwrap(), BASE);
}

#[test]
fn adobe_rgb_progressive_draws_true_luminance_and_ycbcr_stays_unchanged() {
    for al in [0u8, 1u8] {
        // Distinct quantizers per channel verify each RGB channel uses its own DQT entry.
        let image = ColorJpeg {
            width: 64, height: 16,
            blocks: vec![[0, 240, 0], [0, 0, 240], [240, 0, 0], [0, 208, 208],
                         [32, 224, 96], [240, 240, 240], [16, 16, 16], [16, 176, 240]],
            quants: [1, 2, 2],
            progressive: Some(al),
            ..ColorJpeg::default()
        };
        let rgb = render_progressive(&image.bytes()).unwrap();
        assert_eq!(rgb.0.len(), 16);
        // Corners (0, 0) = block 0 ([0, 240, 0]) and (63, 0) = block 7 ([16, 176, 240]).
        let want0 = expected_gray(image.blocks[0]);
        let want7 = expected_gray(image.blocks[7]);
        assert!(rgb.0[0][0].abs_diff(want0) <= 2, "al={al}: got {}, want {want0}", rgb.0[0][0]);
        assert!(rgb.0[0][63].abs_diff(want7) <= 2, "al={al}: got {}, want {want7}", rgb.0[0][63]);

        // Same file with APP14 transform 1 or no APP14 stays Y-only (component 0 = 0 at corner 0).
        for app14 in [Some(adobe_app14(1)), None] {
            let ycbcr = ColorJpeg { app14, ..image.clone() };
            let grey = render_progressive(&ycbcr.bytes()).unwrap();
            assert_eq!(grey.0[0][0], image.blocks[0][0], "al={al} YCbCr corner 0");
            assert_eq!(grey.0[0][63], image.blocks[7][0], "al={al} YCbCr corner 7");
        }
    }
}

#[test]
fn unsupported_adobe_rgb_forms_are_refused_without_drawing() {
    // Non-interleaved baseline and progressive Adobe RGB (1- or 2-component scan)
    // cannot compute RGB within the decoder's memory plan and fail as Unsupported.
    let split_base = ColorJpeg { scan_len: 1, ..ColorJpeg::default() };
    assert!(matches!(render_cover(&split_base.bytes(), &mut Grey::default()),
                     Err(RenderError::Status(BookStatus::Unsupported))));
    for scan_len in [1usize, 2usize] {
        let split = ColorJpeg { progressive: Some(0), scan_len, ..ColorJpeg::default() };
        assert_eq!(render_progressive(&split.bytes()).err(), Some(BookStatus::Unsupported));
    }

    // Adobe transform 2 (YCCK) on a 3-component JPEG is refused in both baseline and progressive.
    let bad_base = ColorJpeg { app14: Some(adobe_app14(2)), ..ColorJpeg::default() };
    assert!(matches!(render_cover(&bad_base.bytes(), &mut Grey::default()),
                     Err(RenderError::Status(BookStatus::Unsupported))));
    let bad_prog = ColorJpeg { app14: Some(adobe_app14(2)), progressive: Some(0), ..ColorJpeg::default() };
    assert_eq!(render_progressive(&bad_prog.bytes()).err(), Some(BookStatus::Unsupported));

    // Subsampled baseline RGB whose first SOS component is not component 0 is refused.
    let bad_order = ColorJpeg { sampling: 0x22, scan_order: [1, 0, 2], ..ColorJpeg::default() };
    assert!(matches!(render_cover(&bad_order.bytes(), &mut Grey::default()),
                     Err(RenderError::Status(BookStatus::Unsupported))));
}

#[test]
fn adobe_transform_255_baseline_is_unsupported_with_zero_rows() {
    let image = ColorJpeg { app14: Some(adobe_app14(0xff)), ..ColorJpeg::default() };
    let mut grey = Grey::default();
    assert!(matches!(render_cover(&image.bytes(), &mut grey),
                     Err(RenderError::Status(BookStatus::Unsupported))));
    assert!(grey.0.is_empty());
}

#[test]
fn adobe_transform_255_progressive_is_unsupported_with_zero_rows() {
    let image = ColorJpeg { app14: Some(adobe_app14(0xff)), progressive: Some(0), ..ColorJpeg::default() };
    let mut grey = Grey::default();
    assert_eq!(render_progressive_into(&image.bytes(), &mut grey), Err(BookStatus::Unsupported));
    assert!(grey.0.is_empty());
}
