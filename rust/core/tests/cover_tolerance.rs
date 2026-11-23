//! #316: the cover decoder accepts what the frozen C++ (TJpgDec) drew, and old
//! Rust no-art records are read once more. Payload CRCs below are the frozen
//! C++ cover session's rasters for these exact bytes (stored in an EPUB).
use newt_core::cover::{self, Record, jpeg::{Layout, Status, baseline, dither_row, placement}};
use newt_core::jpeg::{Error, Input, Workspace, renderer::{RenderError, Rows, render}};
use newt_core::page_renderer::{ImageRows, SafetyError};
use newt_core::zip_window::source::BookSource;

/// Authored grayscale baseline JPEG: quantizers 1, DC category k coded as 4-bit k,
/// AC holds only EOB (`00`). Each 8x8 block is flat at its DC value.
#[derive(Clone)]
struct Jpeg {
    width: u16,
    height: u16,
    dc: Vec<i32>,
    dc_counts: [u8; 16],
    dc_symbols: Vec<u8>,
    sos_tail: Vec<u8>,
}
impl Default for Jpeg {
    fn default() -> Self {
        let mut dc_counts = [0; 16];
        dc_counts[3] = 12;
        Self { width: 64, height: 48, dc: vec![0, 64, 128, -64, 192, -128, 256, -192, 512],
               dc_counts, dc_symbols: (0..12).collect(), sos_tail: vec![0, 63, 0] }
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
        segment(&mut out, 0xc0, &[8, h0, h1, w0, w1, 1, 1, 0x11, 0]);
        let mut ac_counts = [0; 16];
        ac_counts[1] = 1;
        segment(&mut out, 0xc4, &[[0x00].as_slice(), &self.dc_counts, &self.dc_symbols, &[0x10], &ac_counts, &[0]].concat());
        segment(&mut out, 0xda, &[[1, 1, 0x00].as_slice(), &self.sos_tail].concat());
        let mut bits: Vec<bool> = Vec::new();
        let mut put = |value: u32, length: u32| bits.extend((0..length).rev().map(|i| value >> i & 1 != 0));
        let blocks = usize::from(self.width.div_ceil(8)) * usize::from(self.height.div_ceil(8));
        let mut previous = 0;
        for block in 0..blocks {
            let target = self.dc[block % self.dc.len()];
            let diff = target - previous;
            previous = target;
            let category = 32 - diff.unsigned_abs().leading_zeros();
            put(category, 4);
            put((if diff < 0 { diff + (1 << category) - 1 } else { diff }) as u32, category);
            put(0, 2);
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
    assert!(baseline(&mut Bytes(image), &mut layout) == Status::Ready);
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
    assert!(baseline(&mut Bytes(image), &mut layout) == Status::Ready);
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
    let mut progressive = base.clone();
    progressive[sof + 1] = 0xc2;
    let mut cmyk = base.clone();
    cmyk[sof + 9] = 4;
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".as_slice();
    for image in [progressive.as_slice(), &cmyk, png] {
        assert!(baseline(&mut Bytes(image), &mut Layout::default()) == Status::Unsupported);
    }
}

#[test]
fn old_no_art_records_retry_once() {
    // Before #316 Rust wrote CJB4: generate once more, show no cover meanwhile.
    assert_eq!(cover::record(None, cover::RETRIED_NO_ART), Record::Generate);
    assert!(cover::no_art(cover::RETRIED_NO_ART));
    // The record a new attempt writes settles: no repeat.
    let header = cover::header(1234, 5678, cover::NO_ART);
    let written = u32::from_le_bytes(header[24..].try_into().unwrap());
    assert_eq!(cover::record(None, written), Record::Current);
    assert!(cover::no_art(written));
    assert_eq!(cover::record(None, cover::LEGACY_NO_ART), Record::Migrate);
    // A route value is never a picture, and art stays valid across routes.
    assert_eq!(cover::record(Some(BASE), cover::NO_ART), Record::Generate);
    assert_eq!(cover::record(Some(BASE), BASE), Record::Current);
    assert_eq!(cover::record(None, BASE), Record::Generate);
}
