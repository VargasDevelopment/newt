use newt_core::page_renderer::*;
use std::{
    cell::RefCell,
    io::{self, Read},
    rc::Rc,
};
type Trace = Rc<RefCell<Vec<String>>>;
struct Input<'a>(std::str::Split<'a, char>);
impl<'a> Input<'a> {
    fn token(&mut self) -> Result<&'a str, ()> {
        self.0.next().filter(|s| !s.is_empty()).ok_or(())
    }
    fn n(&mut self, lo: i64, hi: i64) -> Result<i64, ()> {
        let s = self.token()?;
        let v = s.parse::<i64>().map_err(|_| ())?;
        if v < lo || v > hi || v.to_string() != s {
            return Err(());
        }
        Ok(v)
    }
    fn hex(&mut self, max: usize) -> Result<Vec<u8>, ()> {
        let s = self.token()?;
        if s == "-" {
            return Ok(Vec::new());
        }
        if s.len() % 2 != 0
            || s.len() > max * 2
            || !s
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
            .collect()
    }
}
struct Fake {
    id: usize,
    max: u32,
    missing: u32,
    raster_missing: u32,
    adv: i16,
    bitmap_adv: i16,
    kern: i16,
    lig: u32,
    w: u16,
    h: u16,
    x: i16,
    y: i16,
    pixels: Vec<u8>,
    trace: Trace,
}
impl Fake {
    fn log(&self, s: String) {
        self.trace.borrow_mut().push(format!("{}:{}", s, self.id));
    }
}
impl Font for Fake {
    fn advance(&mut self, cp: u32, s: u16, f: u8) -> i16 {
        self.log(format!("a{cp}:{s}:{f}"));
        self.adv
    }
    fn kerning(&mut self, l: u32, r: u32, s: u16, f: u8) -> i16 {
        self.log(format!("k{l}:{r}:{s}:{f}"));
        self.kern
    }
    fn ligature(&mut self, l: u32, r: u32, f: u8) -> u32 {
        self.log(format!("g{l}:{r}:{f}"));
        self.lig
    }
    fn line_height(&mut self, s: u16) -> i16 {
        self.log(format!("l{s}"));
        s as i16 + 1
    }
    fn ascent(&mut self, s: u16) -> i16 {
        self.log(format!("s{s}"));
        s as i16 - 1
    }
    fn covers(&mut self, c: u32) -> bool {
        self.has_glyph(c)
    }
}
impl RasterFont for Fake {
    fn has_glyph(&mut self, c: u32) -> bool {
        self.log(format!("h{c}"));
        c <= self.max && c != self.missing
    }
    fn rasterize(&mut self, c: u32, s: u16) -> Option<GlyphBitmap<'_>> {
        self.log(format!("r{c}:{s}"));
        if c > self.max || c == self.missing || c == self.raster_missing {
            return None;
        }
        Some(GlyphBitmap {
            pixels: &self.pixels,
            width: self.w,
            height: self.h,
            xoff: self.x,
            yoff: self.y,
            advance: self.bitmap_adv,
        })
    }
}
struct Decoded {
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    status: BookStatus,
    extra: usize,
    rows: Vec<(u16, u16, Vec<u8>)>,
}
struct Decoder<'a> {
    images: &'a [Decoded],
    trace: Trace,
    index: usize,
}
impl ImageDecoder for Decoder<'_> {
    fn render(
        &mut self,
        _: &PageImage<'_>,
        scratch: &mut ByteArena<'_>,
        sink: &mut dyn ImageRows,
    ) -> Result<BookStatus, SafetyError> {
        let index = self.index;
        self.index += 1;
        let image = &self.images[index];
        self.trace
            .borrow_mut()
            .push(format!("d{index}:{}", scratch.remaining()));
        if scratch.allocate(image.extra, 1).is_none() {
            self.trace.borrow_mut().push(format!("e{index}:7"));
            return Ok(BookStatus::OutOfMemory);
        }
        for (y, w, b) in &image.rows {
            self.trace.borrow_mut().push(format!("i{index}:{y}:{w}"));
            if !sink.on_row(*y, b, *w)? {
                break;
            }
        }
        self.trace
            .borrow_mut()
            .push(format!("e{index}:{}", image.status as u8));
        Ok(image.status)
    }
}
fn status(n: i64) -> BookStatus {
    match n {
        0 => BookStatus::Ok,
        1 => BookStatus::IoError,
        2 => BookStatus::NotZip,
        3 => BookStatus::NotEpub,
        4 => BookStatus::Encrypted,
        5 => BookStatus::Truncated,
        6 => BookStatus::Unsupported,
        7 => BookStatus::OutOfMemory,
        8 => BookStatus::ParseError,
        9 => BookStatus::NotFound,
        _ => BookStatus::Stale,
    }
}
fn run(raw: &str) -> Result<String, ()> {
    if raw.len() > 65536
        || !raw.ends_with('\n')
        || raw[..raw.len() - 1]
            .bytes()
            .any(|c| c != b' ' && !(33..=126).contains(&c))
    {
        return Err(());
    }
    let mut t = Input(raw[..raw.len() - 1].split(' '));
    if t.token()? != "PR1" {
        return Err(());
    }
    let mode = t.n(0, 3)?;
    let w = t.n(1, 64)? as i16;
    let h = t.n(1, 64)? as i16;
    let stride = t.n(1, 64)? as i16;
    let format = match t.n(0, 2)? {
        0 => FrameFormat::Mono1Dithered,
        1 => FrameFormat::Mono1Sharp,
        _ => FrameFormat::Gray8,
    };
    let rotation = match t.n(0, 3)? {
        0 => FrameRotation::None,
        1 => FrameRotation::Portrait,
        2 => FrameRotation::PortraitInverted,
        _ => FrameRotation::UpsideDown,
    };
    let fill = t.n(0, 255)? as u8;
    let capacity = t.n(0, 4096)? as usize;
    let trace = Rc::new(RefCell::new(Vec::new()));
    let nf = t.n(0, 8)?;
    let mut fonts = Vec::new();
    for id in 0..nf as usize {
        let max = t.n(0, 2097151)? as u32;
        let missing = t.n(0, 2097151)? as u32;
        let raster_missing = t.n(0, 2097151)? as u32;
        let adv = t.n(-64, 64)? as i16;
        let bitmap_adv = t.n(-64, 64)? as i16;
        let kern = t.n(-64, 64)? as i16;
        let lig = t.n(0, 2097151)? as u32;
        let w = t.n(0, 8)? as u16;
        let h = t.n(0, 8)? as u16;
        let x = t.n(-64, 64)? as i16;
        let y = t.n(-64, 64)? as i16;
        let pixels = t.hex(64)?;
        if pixels.len() != usize::from(w) * usize::from(h) {
            return Err(());
        }
        fonts.push(Fake {
            id,
            max,
            missing,
            raster_missing,
            adv,
            bitmap_adv,
            kern,
            lig,
            w,
            h,
            x,
            y,
            pixels,
            trace: trace.clone(),
        });
    }
    let ne = t.n(0, 8)?;
    let mut entries = Vec::new();
    for _ in 0..ne {
        entries.push((t.n(0, 7)? as usize, t.n(0, 255)? as u8));
    }
    if entries.iter().any(|e| e.0 >= fonts.len()) {
        return Err(());
    }
    let nr = t.n(0, 32)?;
    let mut run_data = Vec::new();
    for _ in 0..nr {
        let x = t.n(-64, 64)? as i16;
        let y = t.n(-64, 64)? as i16;
        let size = t.n(0, 64)? as u16;
        let style = t.n(0, 255)? as u8;
        let bytes = t.hex(64)?;
        run_data.push((x, y, size, style, bytes));
    }
    let ni = t.n(0, 16)?;
    let mut decoded = Vec::new();
    for _ in 0..ni {
        let x = t.n(-64, 64)? as i16;
        let y = t.n(-64, 64)? as i16;
        let w = t.n(0, 64)? as u16;
        let h = t.n(0, 64)? as u16;
        let s = status(t.n(0, 10)?);
        let extra = t.n(0, 4096)? as usize;
        let n = t.n(0, 64)?;
        let mut rows = Vec::new();
        for _ in 0..n {
            let y = t.n(0, 65535)? as u16;
            let w = t.n(0, 64)? as u16;
            let b = t.hex(64)?;
            if b.len() != usize::from(w) {
                return Err(());
            }
            rows.push((y, w, b));
        }
        decoded.push(Decoded {
            x,
            y,
            w,
            h,
            status: s,
            extra,
            rows,
        });
    }
    let nq = t.n(0, 64)?;
    let mut queries = Vec::new();
    for _ in 0..nq {
        queries.push((
            t.n(0, 7)?,
            t.n(0, 2097151)? as u32,
            t.n(0, 2097151)? as u32,
            t.n(0, 64)? as u16,
            t.n(0, 255)? as u8,
        ));
    }
    if t.0.next().is_some() {
        return Err(());
    }
    let runs: Vec<_> = run_data
        .iter()
        .map(|(x, y, s, f, b)| PageTextRun {
            text: b,
            x: *x,
            baseline_y: *y,
            size_px: *s,
            style_flags: *f,
        })
        .collect();
    let images: Vec<_> = decoded
        .iter()
        .map(|d| PageImage {
            href: b"synthetic",
            x: d.x,
            y: d.y,
            width: d.w,
            height: d.h,
        })
        .collect();
    let page = Page {
        runs: &runs,
        images: &images,
        links: &[],
        page_index: 0,
        char_start: 0,
    };
    let mut bank: Vec<&mut dyn RasterFont> =
        fonts.iter_mut().map(|f| f as &mut dyn RasterFont).collect();
    let mut chain = FontChain::new(&mut bank);
    for (id, flags) in entries {
        if !chain.add(id, flags) {
            return Err(());
        }
    }
    let mut buffer = vec![fill; stride as usize * h as usize];
    let mut frame = FrameTarget {
        framebuffer: &mut buffer,
        width: w,
        height: h,
        width_bytes: stride,
        format,
        rotation,
    };
    frame.validate().map_err(|_| ())?;
    let mut arena = vec![0u8; capacity];
    let mut err = vec![123i16; 66];
    let mut next = vec![-123i16; 66];
    let mut scratch = ImageScratch {
        err: &mut err,
        err_next: &mut next,
        arena: ByteArena::new(&mut arena),
    };
    let mut decoder = Decoder {
        images: &decoded,
        trace: trace.clone(),
        index: 0,
    };
    let mut first = 999;
    let mut missing = 0;
    let mut result = BookStatus::Ok;
    match mode {
        0 => {
            missing =
                render_text(&page, &mut chain, &mut frame, Some(&mut first)).map_err(|_| ())?
        }
        1 => {
            result = render_images(&page, &mut decoder, &mut scratch, &mut frame).map_err(|_| ())?
        }
        2 => {
            result =
                render(&page, &mut chain, &mut decoder, &mut scratch, &mut frame).map_err(|_| ())?
        }
        _ => {}
    }
    let mut answers = Vec::new();
    for (op, c, r, s, f) in queries {
        let v = match op {
            0 => i64::from(chain.advance(c, s, f)),
            1 => i64::from(chain.kerning(c, r, s, f)),
            2 => i64::from(chain.ligature(c, r, f)),
            3 => i64::from(chain.covers(c)),
            4 => i64::from(chain.line_height(s)),
            5 => i64::from(chain.ascent(s)),
            6 => chain.font_for(c, f).map_or(-1, |s| {
                (s.bank_index * 256 + usize::from(s.face_flags)) as i64
            }),
            _ => i64::from(chain.style_coverage()),
        };
        answers.push(v.to_string());
    }
    let hex = buffer
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let trace = trace.borrow();
    Ok(format!(
        "R {missing} {first} {} {hex}\nT {}\nQ {}\n",
        result as u8,
        if trace.is_empty() {
            "-".to_owned()
        } else {
            trace.join(",")
        },
        if answers.is_empty() {
            "-".to_owned()
        } else {
            answers.join(" ")
        }
    ))
}
fn main() {
    let mut raw = String::new();
    if io::stdin().take(65537).read_to_string(&mut raw).is_err() {
        std::process::exit(2);
    }
    match run(&raw) {
        Ok(s) => print!("{s}"),
        Err(()) => std::process::exit(2),
    }
}
