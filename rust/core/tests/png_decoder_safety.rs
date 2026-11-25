use newt_core::{
    image_renderer::*,
    inflate::{BitBuffer, State},
    page_renderer::{ImageRows, SafetyError},
    pngle::*,
};
struct MathRequired;
impl Math for MathRequired {
    fn pow(&self, _: f64, _: f64) -> f64 {
        panic!("gamma math must not be called without enabled gAMA")
    }
}
#[derive(Default)]
struct Pixels {
    init: usize,
    done: usize,
    pixels: Vec<(u32, u32, [u8; 4])>,
}
impl Callbacks for Pixels {
    fn init(&mut self, _: Header) -> Result<(), CallbackError> {
        self.init += 1;
        Ok(())
    }
    fn draw(&mut self, x: u32, y: u32, _w: u32, _h: u32, p: [u8; 4]) -> Result<(), CallbackError> {
        self.pixels.push((x, y, p));
        Ok(())
    }
    fn done(&mut self) -> Result<(), CallbackError> {
        self.done += 1;
        Ok(())
    }
}
#[test]
fn actual_png_decodes_in_both_tinfl_modes_and_admission_can_resume() {
    for mode in [BitBuffer::Bits32, BitBuffer::Bits64] {
        let bytes = include_bytes!("../../tests/png_decoder/tests/fixtures/tiny.png");
        let mut d = Decoder::new();
        let mut inf = State::new(mode);
        let mut p = Pixels::default();
        let mut ring = [0; 32];
        let mut palette = [0; 768];
        let mut trans = [0; 256];
        let mut gamma = [];
        let mut dict = [0; 32768];
        let mut b = Buffers {
            ring: &mut [],
            palette: &mut palette,
            transparency: &mut trans,
            gamma: &mut gamma,
            dictionary: &mut dict,
        };
        let consumed = match d.feed(bytes, &mut b, &mut inf, &MathRequired, &mut p) {
            Err(Error::Capacity {
                buffer: Buffer::Ring,
                needed,
                consumed,
            }) => {
                assert_eq!(needed, 16);
                assert_eq!(consumed, 16);
                consumed
            }
            v => panic!("{v:?}"),
        };
        b.ring = &mut ring;
        assert_eq!(
            d.feed(&bytes[consumed..], &mut b, &mut inf, &MathRequired, &mut p),
            Ok(bytes.len() - consumed)
        );
        assert!(d.eof());
        assert_eq!((p.init, p.done, p.pixels.len()), (1, 1, 4));
        assert_eq!(p.pixels[0], (0, 0, [10, 20, 30, 255]));
        assert_eq!(p.pixels[3], (1, 1, [100, 110, 120, 0]));
    }
}
#[test]
fn crc_failure_sticks_and_reset_releases_it() {
    let mut bytes = include_bytes!("../../tests/png_decoder/tests/fixtures/tiny.png").to_vec();
    bytes[29] ^= 1;
    let mut d = Decoder::new();
    let mut inf = State::new(BitBuffer::Bits32);
    let mut p = Pixels::default();
    let mut ring = [0; 32];
    let mut dict = [0; 32768];
    let mut b = Buffers {
        ring: &mut ring,
        palette: &mut [],
        transparency: &mut [],
        gamma: &mut [],
        dictionary: &mut dict,
    };
    assert_eq!(
        d.feed(&bytes, &mut b, &mut inf, &MathRequired, &mut p),
        Err(Error::Crc)
    );
    assert_eq!(
        d.feed(&[0], &mut b, &mut inf, &MathRequired, &mut p),
        Err(Error::Crc)
    );
    d.reset(&mut inf);
    assert_eq!(
        d.feed(
            include_bytes!("../../tests/png_decoder/tests/fixtures/tiny.png"),
            &mut b,
            &mut inf,
            &MathRequired,
            &mut p
        ),
        Ok(bytes.len())
    );
}
struct Rows {
    rows: Vec<Vec<u8>>,
    stop: usize,
}
impl ImageRows for Rows {
    fn on_row(&mut self, _: u16, row: &[u8], _: u16) -> Result<bool, SafetyError> {
        self.rows.push(row.to_vec());
        Ok(self.rows.len() != self.stop)
    }
}
#[test]
fn scaler_counts_every_source_sample_and_preserves_stop() {
    let mut row = [0; 1];
    let mut sums = [0; 1];
    let mut counts = [0; 1];
    let mut sink = Rows {
        rows: vec![],
        stop: 1,
    };
    let mut s = RowScaler::new(3, 2, 1, 1, &mut row, &mut sums, &mut counts).unwrap();
    assert!(s.feed(0, &[0, 60, 120], &mut sink).unwrap());
    assert!(!s.feed(1, &[180, 240, 252], &mut sink).unwrap());
    assert!(s.stopped);
    assert_eq!(sink.rows, [vec![142]]);
}
#[test]
fn alpha_is_blended_over_white_after_luma_rounding() {
    assert_eq!(rgba_to_gray([0, 0, 0, 0]), 255);
    assert_eq!(rgba_to_gray([0, 0, 0, 128]), 127);
    assert_eq!(rgba_to_gray([255, 0, 0, 255]), 76);
}
struct Source<'a>(&'a [u8]);
impl newt_core::zip_window::source::BookSource for Source<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Ok(at) = usize::try_from(at) else {
            return -1;
        };
        let Some(tail) = self.0.get(at..) else {
            return -1;
        };
        let n = out.len().min(tail.len());
        out[..n].copy_from_slice(&tail[..n]);
        n as i32
    }
}
struct Lookup(newt_core::member_check::Checked);
impl EntryLookup for Lookup {
    fn find_entry(&self, href: &[u8]) -> Option<newt_core::member_check::Checked> {
        (href == b"image.png").then_some(self.0)
    }
}
#[test]
fn page_decoder_uses_real_probe_reader_and_png_bytes() {
    use newt_core::{
        page_renderer::{BookStatus, ByteArena, ImageDecoder, PageImage},
        member_check::Checked,
    };
    let bytes = include_bytes!("../../tests/png_decoder/tests/fixtures/tiny.png");
    let mut source = Source(bytes);
    let lookup = Lookup(Checked::raw(bytes.len() as u32));
    let mut zip_state = State::new(BitBuffer::Bits32);
    let mut png_state = State::new(BitBuffer::Bits32);
    let mut decoder = Decoder::new();
    let mut zip_dict = [0; 32768];
    let mut zip_input = [0; newt_core::zip_container::INPUT_SIZE];
    let mut skip = [0; 1024];
    let mut dict = [0; 32768];
    let mut ring = [0; 16];
    let mut src = [0; 2];
    let mut dst = [0; 2];
    let mut sums = [0; 2];
    let mut counts = [0; 2];
    let mut read = [0; 4096];
    let mut d = ZipPngDecoder {
        source: &mut source,
        catalog: &lookup,
        zip_state: &mut zip_state,
        zip_dictionary: &mut zip_dict,
        zip_input: &mut zip_input,
        probe_skip: &mut skip,
        decoder: &mut decoder,
        png_state: &mut png_state,
        png: Buffers {
            ring: &mut ring,
            palette: &mut [],
            transparency: &mut [],
            gamma: &mut [],
            dictionary: &mut dict,
        },
        work: RenderBuffers {
            src: &mut src,
            read: &mut read,
            dst: &mut dst,
            sums: &mut sums,
            counts: &mut counts,
        },
        math: &MathRequired,
    };
    let image = PageImage {
        href: b"image.png",
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };
    let mut scratch = ByteArena::new(&mut []);
    let mut rows = Rows {
        rows: vec![],
        stop: 0,
    };
    for _ in 0..2 {
        rows.rows.clear();
        assert_eq!(
            d.render(&image, &mut scratch, &mut rows),
            Ok(BookStatus::Ok)
        );
        assert_eq!(rows.rows, [vec![18, 152], vec![78, 255]]);
    }
}

#[test]
fn pinned_background_chunk_spelling_is_preserved() {
    for (bytes, expected) in [
        (
            &include_bytes!("../../tests/png_decoder/tests/fixtures/indexed-background.png")[..],
            [0, 0, 0],
        ),
        (
            &include_bytes!("../../tests/png_decoder/tests/fixtures/native-uppercase-background.png")[..],
            [128, 128, 128],
        ),
    ] {
        let mut decoder = Decoder::new();
        let mut inflate = State::new(BitBuffer::Bits32);
        let mut ring = [0; 512];
        let mut palette = [0; 768];
        let mut trans = [0; 256];
        let mut gamma = [];
        let mut dictionary = [0; 32768];
        let mut buffers = Buffers {
            ring: &mut ring,
            palette: &mut palette,
            transparency: &mut trans,
            gamma: &mut gamma,
            dictionary: &mut dictionary,
        };
        let mut pixels = Pixels::default();
        assert_eq!(
            decoder.feed(
                bytes,
                &mut buffers,
                &mut inflate,
                &MathRequired,
                &mut pixels
            ),
            Ok(bytes.len())
        );
        assert!(decoder.eof());
        assert_eq!(decoder.background, expected);
    }
}

#[test]
fn callback_failures_are_sticky_and_never_buffer_admission() {
    struct Reject {
        at: u8,
        calls: u32,
    }
    impl Callbacks for Reject {
        fn init(&mut self, _: Header) -> Result<(), CallbackError> {
            self.calls += 1;
            if self.at == 0 {
                Err(CallbackError)
            } else {
                Ok(())
            }
        }
        fn draw(
            &mut self,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: [u8; 4],
        ) -> Result<(), CallbackError> {
            self.calls += 1;
            if self.at == 1 {
                Err(CallbackError)
            } else {
                Ok(())
            }
        }
        fn done(&mut self) -> Result<(), CallbackError> {
            self.calls += 1;
            if self.at == 2 {
                Err(CallbackError)
            } else {
                Ok(())
            }
        }
    }
    for at in 0..3 {
        let mut d = Decoder::new();
        let mut state = State::new(BitBuffer::Bits32);
        let mut cb = Reject { at, calls: 0 };
        let mut ring = [0; 32];
        let mut palette = [];
        let mut trans = [0; 256];
        let mut gamma = [];
        let mut dictionary = [0; 32768];
        let mut buffers = Buffers {
            ring: &mut ring,
            palette: &mut palette,
            transparency: &mut trans,
            gamma: &mut gamma,
            dictionary: &mut dictionary,
        };
        assert_eq!(
            d.feed(
                include_bytes!("../../tests/png_decoder/tests/fixtures/tiny.png"),
                &mut buffers,
                &mut state,
                &MathRequired,
                &mut cb
            ),
            Err(Error::Callback)
        );
        let calls = cb.calls;
        buffers.ring = &mut [];
        assert_eq!(
            d.feed(b"retry", &mut buffers, &mut state, &MathRequired, &mut cb),
            Err(Error::Callback)
        );
        assert_eq!(cb.calls, calls);
    }
}

#[test]
fn native_background_stalls_before_undefined_complete_body() {
    let mut d = Decoder::new();
    let mut state = State::new(BitBuffer::Bits32);
    let mut pixels = Pixels::default();
    let mut ring = [0; 32];
    let mut palette = [];
    let mut trans = [];
    let mut gamma = [];
    let mut dictionary = [0; 32768];
    let mut buffers = Buffers {
        ring: &mut ring,
        palette: &mut palette,
        transparency: &mut trans,
        gamma: &mut gamma,
        dictionary: &mut dictionary,
    };
    let empty = include_bytes!("../../tests/png_decoder/tests/fixtures/empty-native-background.png");
    assert_eq!(
        d.feed(empty, &mut buffers, &mut state, &MathRequired, &mut pixels),
        Ok(empty.len())
    );
    assert!(d.eof());
    d.reset(&mut state);
    let fragment = include_bytes!("../../tests/png_decoder/tests/fixtures/fragmented-native-background.png");
    assert_eq!(
        d.feed(
            fragment,
            &mut buffers,
            &mut state,
            &MathRequired,
            &mut pixels
        ),
        Ok(41)
    );
    assert_eq!(
        d.feed(
            &fragment[41..],
            &mut buffers,
            &mut state,
            &MathRequired,
            &mut pixels
        ),
        Ok(0)
    );
    assert_eq!(
        d.feed(
            &[0; 6],
            &mut buffers,
            &mut state,
            &MathRequired,
            &mut pixels
        ),
        Err(Error::NativeUndefinedBackground)
    );
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend((body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(body);
    let crc = newt_core::cover::crc(u32::MAX, &out[start..]) ^ u32::MAX;
    out.extend(crc.to_be_bytes());
}

fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let len = raw.len() as u16;
    out.push(1);
    out.extend(len.to_le_bytes());
    out.extend((!len).to_le_bytes());
    out.extend(raw);
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend(((b << 16) | a).to_be_bytes());
    out
}

fn rgb_4x4_pixels() -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut scanlines = Vec::new();
    let mut expected = Vec::new();
    for y in 0..4u8 {
        scanlines.push(0);
        let mut row = Vec::new();
        for x in 0..4u8 {
            let rgb = [y * 50 + x * 11, 20 + x * 47, 220 - y * 41 - x * 13];
            scanlines.extend(rgb);
            row.push(rgba_to_gray([rgb[0], rgb[1], rgb[2], 255]));
        }
        expected.push(row);
    }
    (scanlines, expected)
}

fn padded_rgb_4x4_png(before_idat_pad: usize, after_idat_pad: Option<usize>) -> Vec<u8> {
    let (scanlines, _) = rgb_4x4_pixels();
    let mut out = newt_core::cover::png::SIGNATURE.to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(4u32.to_be_bytes());
    ihdr.extend(4u32.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"tEXt", &vec![b'a'; before_idat_pad]);
    png_chunk(&mut out, b"IDAT", &zlib_stored(&scanlines));
    if let Some(after_pad) = after_idat_pad {
        png_chunk(&mut out, b"tEXt", &vec![b'b'; after_pad]);
    }
    png_chunk(&mut out, b"IEND", &[]);
    out
}

struct SteppedInput<'a> {
    bytes: &'a [u8],
    at: usize,
    step: usize,
    reads: usize,
    max_reads: usize,
}
impl EntryInput for SteppedInput<'_> {
    fn read(&mut self, dst: &mut [u8]) -> i32 {
        self.reads += 1;
        assert!(
            self.reads <= self.max_reads,
            "render_png exceeded read bound ({})",
            self.max_reads
        );
        if self.at >= self.bytes.len() {
            return 0;
        }
        let n = dst.len().min(self.step).min(self.bytes.len() - self.at);
        dst[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        n as i32
    }
}

fn decode_with_step(
    bytes: &[u8],
    w: u16,
    h: u16,
    step: usize,
    max_reads: usize,
) -> (newt_core::page_renderer::BookStatus, Vec<Vec<u8>>, usize) {
    let mut input = SteppedInput { bytes, at: 0, step, reads: 0, max_reads };
    let (mut decoder, mut inflate) = (Decoder::new(), State::new(BitBuffer::Bits32));
    let (mut ring, mut palette, mut trans, mut dict) = ([0; 64], [0; 768], [0; 256], [0; 32768]);
    let mut png = Buffers {
        ring: &mut ring, palette: &mut palette, transparency: &mut trans,
        gamma: &mut [], dictionary: &mut dict,
    };
    let n = usize::from(w);
    let (mut src, mut read, mut dst, mut sums, mut counts) = (vec![0; n], [0; 4096], vec![0; n], vec![0; n], vec![0; n]);
    let mut sink = Rows { rows: vec![], stop: 0 };
    let status = render_png(
        &mut input, w, h, w, h, &mut decoder, &mut inflate, &mut png,
        RenderBuffers { src: &mut src, read: &mut read, dst: &mut dst, sums: &mut sums, counts: &mut counts },
        &MathRequired, &mut sink,
    )
    .unwrap();
    (status, sink.rows, input.reads)
}

#[test]
fn render_png_carries_chunk_header_and_crc_across_4096_boundary() {
    use newt_core::{
        member_check::Checked,
        page_renderer::{BookStatus, ByteArena, ImageDecoder, PageImage},
    };
    let (_, expected) = rgb_4x4_pixels();
    // Signature (8) + IHDR (25) = 33; tEXt overhead = 12 => IDAT starts at 45 + pad.
    // Cover IDAT 8-byte header straddling 4096 (4089..=4095), aligned 4096,
    // and tEXt 4-byte CRC straddling 4096 (IDAT starts at 4097..=4099).
    for idat_offset in 4089..=4099usize {
        let png = padded_rgb_4x4_png(idat_offset - 45, None);
        let (status, rows, _) = decode_with_step(&png, 4, 4, 4096, 8);
        assert_eq!(status, BookStatus::Ok, "idat_offset={idat_offset}");
        assert_eq!(rows, expected, "idat_offset={idat_offset}");
    }
    // Also place IEND header and trailing tEXt CRC across byte 4096.
    let iend_at_zero = padded_rgb_4x4_png(0, Some(0)).len() - 12;
    for iend_offset in 4089..=4099usize {
        let png = padded_rgb_4x4_png(0, Some(iend_offset - iend_at_zero));
        let (status, rows, _) = decode_with_step(&png, 4, 4, 4096, 8);
        assert_eq!(status, BookStatus::Ok, "iend_offset={iend_offset}");
        assert_eq!(rows, expected, "iend_offset={iend_offset}");
    }
    // Verify the full ZipPngDecoder path on both IDAT header (4092) and tEXt CRC (4098).
    for idat_offset in [4092usize, 4098] {
        let bytes = padded_rgb_4x4_png(idat_offset - 45, None);
        let (mut source, lookup) = (Source(&bytes), Lookup(Checked::raw(bytes.len() as u32)));
        let (mut zip_state, mut png_state) = (State::new(BitBuffer::Bits32), State::new(BitBuffer::Bits32));
        let (mut decoder, mut zip_dict) = (Decoder::new(), [0; 32768]);
        let (mut zip_input, mut skip) = ([0; newt_core::zip_container::INPUT_SIZE], [0; 1024]);
        let (mut dict, mut ring) = ([0; 32768], [0; 64]);
        let (mut src, mut dst, mut sums, mut counts, mut read) = ([0; 4], [0; 4], [0; 4], [0; 4], [0; 4096]);
        let mut d = ZipPngDecoder {
            source: &mut source, catalog: &lookup, zip_state: &mut zip_state,
            zip_dictionary: &mut zip_dict, zip_input: &mut zip_input, probe_skip: &mut skip,
            decoder: &mut decoder, png_state: &mut png_state,
            png: Buffers { ring: &mut ring, palette: &mut [], transparency: &mut [], gamma: &mut [], dictionary: &mut dict },
            work: RenderBuffers { src: &mut src, read: &mut read, dst: &mut dst, sums: &mut sums, counts: &mut counts },
            math: &MathRequired,
        };
        let image = PageImage { href: b"image.png", x: 0, y: 0, width: 4, height: 4 };
        let (mut scratch, mut sink) = (ByteArena::new(&mut []), Rows { rows: vec![], stop: 0 });
        assert_eq!(d.render(&image, &mut scratch, &mut sink), Ok(BookStatus::Ok), "zip idat_offset={idat_offset}");
        assert_eq!(sink.rows, expected, "zip idat_offset={idat_offset}");
    }
}

#[test]
fn render_png_handles_short_reads_across_chunk_boundaries() {
    use newt_core::page_renderer::BookStatus;
    let tiny = include_bytes!("../../tests/png_decoder/tests/fixtures/tiny.png");
    let expected_tiny = vec![vec![18, 152], vec![78, 255]];
    // Palette + tRNS 2x2 PNG exercises 3-byte PLTE entries and tRNS across tiny reads.
    let mut indexed = newt_core::cover::png::SIGNATURE.to_vec();
    let ihdr = [2u32.to_be_bytes().as_slice(), &2u32.to_be_bytes(), &[8, 3, 0, 0, 0]].concat();
    png_chunk(&mut indexed, b"IHDR", &ihdr);
    png_chunk(&mut indexed, b"PLTE", &[10, 20, 30, 200, 100, 50]);
    png_chunk(&mut indexed, b"tRNS", &[255, 128]);
    png_chunk(&mut indexed, b"IDAT", &zlib_stored(&[0, 0, 1, 0, 1, 0]));
    png_chunk(&mut indexed, b"IEND", &[]);
    let (c0, c1) = (rgba_to_gray([10, 20, 30, 255]), rgba_to_gray([200, 100, 50, 128]));
    let expected_indexed = vec![vec![c0, c1], vec![c1, c0]];
    for step in [1usize, 2, 3, 5, 7, 13] {
        let (st, rows, _) = decode_with_step(tiny, 2, 2, step, tiny.len() + 4);
        assert_eq!((st, rows), (BookStatus::Ok, expected_tiny.clone()), "tiny step={step}");
        let (st, rows, _) = decode_with_step(&indexed, 2, 2, step, indexed.len() + 4);
        assert_eq!((st, rows), (BookStatus::Ok, expected_indexed.clone()), "indexed step={step}");
    }
}

#[test]
fn render_png_terminates_cleanly_on_stalled_tail_at_eof() {
    use newt_core::page_renderer::BookStatus;
    let idat_split = padded_rgb_4x4_png(4092 - 45, None);
    let crc_split = padded_rgb_4x4_png(4098 - 45, None);
    for (name, slice) in [
        ("empty", &[][..]),
        ("partial-signature", &idat_split[..5]),
        ("partial-idat-header-at-4096", &idat_split[..4096]),
        ("partial-text-crc-at-4096", &crc_split[..4096]),
    ] {
        let (status, _, reads) = decode_with_step(slice, 4, 4, 4096, 4);
        assert_eq!(status, BookStatus::Ok, "{name}");
        assert!(reads <= 2, "{name} reads={reads}");
    }
}

#[test]
fn render_png_rejects_oversized_read_with_held_tail() {
    struct OversizedSecondRead<'a>(&'a [u8], usize);
    impl EntryInput for OversizedSecondRead<'_> {
        fn read(&mut self, dst: &mut [u8]) -> i32 {
            self.1 += 1;
            if self.1 == 1 {
                dst.copy_from_slice(&self.0[..4096]);
                4096
            } else {
                // Report more bytes than dst.len() (4092) when 4 tail bytes are held.
                (dst.len() + 1) as i32
            }
        }
    }
    let bytes = padded_rgb_4x4_png(4092 - 45, None);
    let (mut input, mut decoder, mut inflate) = (OversizedSecondRead(&bytes, 0), Decoder::new(), State::new(BitBuffer::Bits32));
    let (mut ring, mut dict) = ([0; 64], [0; 32768]);
    let mut png = Buffers { ring: &mut ring, palette: &mut [], transparency: &mut [], gamma: &mut [], dictionary: &mut dict };
    let (mut src, mut read, mut dst, mut sums, mut counts) = ([0; 4], [0; 4096], [0; 4], [0; 4], [0; 4]);
    let mut sink = Rows { rows: vec![], stop: 0 };
    assert_eq!(
        render_png(
            &mut input, 4, 4, 4, 4, &mut decoder, &mut inflate, &mut png,
            RenderBuffers { src: &mut src, read: &mut read, dst: &mut dst, sums: &mut sums, counts: &mut counts },
            &MathRequired, &mut sink,
        ),
        Err(SafetyError::RowSlice)
    );
}
