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
