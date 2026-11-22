//! Translation of pinned pngle.c (MIT, kikuchan 2019); see rust/THIRD_PARTY.md.
#![forbid(unsafe_code)]
use crate::inflate::{self, State};
const OX: [u32; 8] = [0, 0, 4, 0, 2, 0, 1, 0];
const OY: [u32; 8] = [0, 0, 0, 4, 0, 2, 0, 1];
const DX: [u32; 8] = [1, 8, 8, 4, 4, 2, 2, 1];
const DY: [u32; 8] = [1, 8, 8, 8, 4, 4, 2, 2];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Buffer {
    Ring,
    Palette,
    Transparency,
    Gamma,
    Dictionary,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Signature,
    Header,
    Order,
    Chunk,
    Crc,
    Inflate,
    Filter,
    PaletteIndex,
    Capacity {
        buffer: Buffer,
        needed: usize,
        consumed: usize,
    },
    Arithmetic,
    Callback,
    NativeUndefinedBackground,
}
/// Callback failure cannot masquerade as resumable decoder buffer admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackError;
impl From<CallbackError> for Error {
    fn from(_: CallbackError) -> Self {
        Self::Callback
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub width: u32,
    pub height: u32,
    pub depth: u8,
    pub color: u8,
    pub compression: u8,
    pub filter: u8,
    pub interlace: u8,
}
/// Implement using approved target math. No approximate or silently disabled gamma.
pub trait Math {
    fn pow(&self, base: f64, exponent: f64) -> f64;
}
pub trait Callbacks {
    fn init(&mut self, header: Header) -> Result<(), CallbackError>;
    fn draw(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: [u8; 4]) -> Result<(), CallbackError>;
    fn done(&mut self) -> Result<(), CallbackError>;
}
pub struct Buffers<'a> {
    pub ring: &'a mut [u8],
    pub palette: &'a mut [u8],
    pub transparency: &'a mut [u8],
    pub gamma: &'a mut [u8],
    pub dictionary: &'a mut [u8],
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initial,
    Header,
    Body,
    Crc,
    Eof,
    Failed,
}
pub struct Decoder {
    pub header: Header,
    phase: Phase,
    kind: [u8; 4],
    remain: u32,
    crc: u32,
    channels: usize,
    palettes: usize,
    palette_size: usize,
    trans_size: usize,
    trans: usize,
    has_palette: bool,
    has_trans: bool,
    started: bool,
    out: usize,
    pass: usize,
    ring_len: usize,
    cursor: usize,
    render_remain: i32,
    filter: i8,
    x: u32,
    y: u32,
    gamma_len: usize,
    pub display_gamma: f64,
    pub background: [u8; 3],
    pub error: Option<Error>,
}
fn admission_at(e: Error, consumed: usize) -> Error {
    match e {
        Error::Capacity { buffer, needed, .. } => Error::Capacity {
            buffer,
            needed,
            consumed,
        },
        other => other,
    }
}
fn u32be(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn need(actual: usize, needed: usize, buffer: Buffer) -> Result<(), Error> {
    if actual < needed {
        Err(Error::Capacity {
            buffer,
            needed,
            consumed: 0,
        })
    } else {
        Ok(())
    }
}
/// Original polynomial, table-free; internal state uses the complemented form.
fn crc(mut c: u32, bytes: &[u8]) -> u32 {
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(c & 1));
        }
    }
    c
}
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i32::from(a) + i32::from(b) - i32::from(c);
    let (pa, pb, pc) = (
        (p - i32::from(a)).abs(),
        (p - i32::from(b)).abs(),
        (p - i32::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}
impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}
impl Decoder {
    pub const fn new() -> Self {
        Self {
            header: Header {
                width: 0,
                height: 0,
                depth: 0,
                color: 0,
                compression: 0,
                filter: 0,
                interlace: 0,
            },
            phase: Phase::Initial,
            kind: [0; 4],
            remain: 0,
            crc: 0,
            channels: 0,
            palettes: 0,
            palette_size: 0,
            trans_size: 0,
            trans: 0,
            has_palette: false,
            has_trans: false,
            started: false,
            out: 0,
            pass: 0,
            ring_len: 0,
            cursor: 0,
            render_remain: -1,
            filter: -1,
            x: 0,
            y: 0,
            gamma_len: 0,
            display_gamma: 0.0,
            background: [0; 3],
            error: None,
        }
    }
    pub fn reset(&mut self, inflate: &mut State) {
        let gamma = self.display_gamma;
        let background = self.background;
        *self = Self::new();
        self.display_gamma = gamma;
        self.background = background;
        inflate.reset();
    }
    pub fn eof(&self) -> bool {
        self.phase == Phase::Eof
    }
    fn maxval(&self) -> u32 {
        (1u32
            << if self.header.color & 1 != 0 {
                8
            } else {
                self.header.depth
            })
            - 1
    }
    fn bpp(&self) -> usize {
        (self.channels * usize::from(self.header.depth) + 7) / 8
    }
    fn ring_size(&self, pass: usize) -> Result<usize, Error> {
        // Empty Adam7 passes contain no filter byte. Avoid native unsigned underflow.
        let pixels =
            u64::from(self.header.width.saturating_sub(OX[pass])).div_ceil(u64::from(DX[pass]));
        let bytes = (pixels * u64::from(self.header.depth) * self.channels as u64).div_ceil(8);
        usize::try_from(bytes)
            .ok()
            .and_then(|n| n.checked_add(self.bpp() * 2))
            .ok_or(Error::Arithmetic)
    }
    fn set_pass(&mut self, pass: usize, buffers: &mut Buffers<'_>) -> Result<(), Error> {
        let len = self.ring_size(pass)?;
        need(buffers.ring.len(), len, Buffer::Ring)?;
        buffers.ring[..len].fill(0);
        self.ring_len = len;
        self.pass = pass;
        self.x = OX[pass];
        self.y = OY[pass];
        self.cursor = 0;
        self.render_remain = -1;
        self.filter = -1;
        Ok(())
    }
    fn sample(&self, ring: &[u8], index: &mut usize, bits: &mut u8) -> u16 {
        match self.header.depth {
            1 | 2 | 4 => {
                if *bits >= 8 {
                    *bits = 0;
                    *index = (*index + 1) % self.ring_len;
                }
                *bits += self.header.depth;
                u16::from((ring[*index] >> (8 - *bits)) & ((1 << self.header.depth) - 1))
            }
            8 => {
                let n = ring[*index];
                *index = (*index + 1) % self.ring_len;
                u16::from(n)
            }
            _ => {
                let n = u16::from(ring[*index]);
                *index = (*index + 1) % self.ring_len;
                let n = n * 256 + u16::from(ring[*index]);
                *index = (*index + 1) % self.ring_len;
                n
            }
        }
    }
    fn color(&self, mut v: [u16; 4], b: &Buffers<'_>) -> Result<[u8; 4], Error> {
        let max = self.maxval();
        let transparent = |v: &[u16]| {
            self.trans == 1
                && v.iter().enumerate().all(|(i, &n)| {
                    n == u16::from_be_bytes([b.transparency[2 * i], b.transparency[2 * i + 1]])
                })
        };
        if self.header.color & 2 != 0 {
            if self.header.color & 1 != 0 {
                let i = usize::from(v[0]);
                if i >= self.palettes {
                    return Err(Error::PaletteIndex);
                }
                v[0] = u16::from(b.palette[3 * i]);
                v[1] = u16::from(b.palette[3 * i + 1]);
                v[2] = u16::from(b.palette[3 * i + 2]);
                v[3] = if i < self.trans {
                    u16::from(b.transparency[i])
                } else {
                    max as u16
                };
            } else if self.header.color & 4 == 0 {
                v[3] = if transparent(&v[..3]) { 0 } else { max as u16 };
            }
        } else {
            v[3] = if self.header.color & 4 != 0 {
                v[1]
            } else if transparent(&v[..1]) {
                0
            } else {
                max as u16
            };
            v[1] = v[0];
            v[2] = v[0];
        }
        let mut rgba = [0; 4];
        for i in 0..4 {
            rgba[i] = ((u32::from(v[i]) * 255 + max / 2) / max) as u8;
        }
        if self.gamma_len != 0 {
            for i in 0..3 {
                rgba[i] = b.gamma[usize::from(v[i])];
            }
        }
        Ok(rgba)
    }
    fn push(&mut self, b: &mut Buffers<'_>, v: u8) {
        b.ring[self.cursor] = v;
        self.cursor = (self.cursor + 1) % self.ring_len;
    }
    fn data_byte(
        &mut self,
        byte: u8,
        b: &mut Buffers<'_>,
        cb: &mut dyn Callbacks,
    ) -> Result<(), Error> {
        let bpp = self.bpp();
        loop {
            if self.x >= self.header.width {
                self.x = OX[self.pass];
                self.y = self.y.saturating_add(DY[self.pass]).min(self.header.height);
                self.filter = -1;
            }
            if self.x < self.header.width && self.y < self.header.height {
                break;
            }
            if self.pass == 0 || self.pass >= 7 {
                return Ok(());
            }
            self.set_pass(self.pass + 1, b)?;
        }
        if self.filter < 0 {
            if byte > 4 {
                return Err(Error::Filter);
            }
            self.filter = byte as i8;
            for _ in 0..bpp {
                self.push(b, 0);
            }
            return Ok(());
        }
        let c = b.ring[self.cursor];
        let up = b.ring[(self.cursor + bpp) % self.ring_len];
        let left = b.ring[(self.cursor + self.ring_len - bpp) % self.ring_len];
        let add = match self.filter {
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            4 => paeth(left, up, c),
            _ => 0,
        };
        self.push(b, byte.wrapping_add(add));
        if self.render_remain < 0 {
            self.render_remain = bpp as i32;
        }
        self.render_remain -= 1;
        if self.render_remain == 0 {
            let mut ix = (self.cursor + self.ring_len - bpp) % self.ring_len;
            let mut bits = 0;
            let pixels = if self.header.depth == 16 {
                1
            } else {
                8 / self.header.depth
            };
            for _ in 0..pixels {
                if self.x >= self.header.width {
                    break;
                }
                let mut v = [0; 4];
                for n in v.iter_mut().take(self.channels) {
                    *n = self.sample(b.ring, &mut ix, &mut bits);
                }
                let rgba = self.color(v, b)?;
                cb.draw(
                    self.x,
                    self.y,
                    (DX[self.pass] - OX[self.pass]).min(self.header.width - self.x),
                    (DY[self.pass] - OY[self.pass]).min(self.header.height - self.y),
                    rgba,
                )?;
                self.x = self.x.saturating_add(DX[self.pass]).min(self.header.width);
            }
            self.render_remain = -1;
        }
        Ok(())
    }
    fn chunk_start(
        &mut self,
        kind: [u8; 4],
        len: u32,
        b: &mut Buffers<'_>,
        cb: &mut dyn Callbacks,
    ) -> Result<(), Error> {
        match &kind {
            b"IHDR" => {
                if len != 13 {
                    return Err(Error::Chunk);
                }
                if self.channels != 0 {
                    return Err(Error::Order);
                }
            }
            b"IDAT" => {
                if len == 0 {
                    return Err(Error::Chunk);
                }
                if self.channels == 0 || (self.header.color == 3 && !self.has_palette) {
                    return Err(Error::Order);
                }
                need(
                    b.dictionary.len(),
                    inflate::DICTIONARY_SIZE,
                    Buffer::Dictionary,
                )?;
                if !self.started {
                    cb.init(self.header)?;
                    self.started = true;
                    self.out = 0;
                }
            }
            b"PLTE" => {
                if self.channels == 0 || self.has_palette {
                    return Err(Error::Order);
                }
                if !matches!(self.header.color, 2 | 3 | 6)
                    || len == 0
                    || len % 3 != 0
                    || len / 3 > 256u32.min(1u32 << self.header.depth)
                {
                    return Err(Error::Chunk);
                }
                need(b.palette.len(), len as usize, Buffer::Palette)?;
                self.palette_size = len as usize;
                self.has_palette = true;
                self.palettes = 0;
            }
            b"tRNS" => {
                if self.channels == 0 || self.has_trans {
                    return Err(Error::Order);
                }
                let valid = match self.header.color {
                    0 => len == 2,
                    2 => len == 6,
                    3 => len > 0 && len <= 1u32 << self.header.depth,
                    _ => false,
                };
                if !valid {
                    return Err(Error::Chunk);
                }
                need(b.transparency.len(), len as usize, Buffer::Transparency)?;
                self.trans_size = len as usize;
                self.has_trans = true;
                self.trans = 0;
            }
            b"IEND" => {
                if !self.started {
                    return Err(Error::Order);
                }
                if len != 0 {
                    return Err(Error::Chunk);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(
        &mut self,
        input: &[u8],
        b: &mut Buffers<'_>,
        inflate: &mut State,
        math: &dyn Math,
        cb: &mut dyn Callbacks,
    ) -> Result<usize, Error> {
        match &self.kind {
            b"IHDR" => {
                if input.len() < 13 {
                    return Ok(0);
                }
                let h = Header {
                    width: u32be(input),
                    height: u32be(&input[4..]),
                    depth: input[8],
                    color: input[9],
                    compression: input[10],
                    filter: input[11],
                    interlace: input[12],
                };
                let channels = match h.color {
                    0 | 3 => 1,
                    2 => 3,
                    4 => 2,
                    6 => 4,
                    _ => return Err(Error::Header),
                };
                if !(matches!(h.depth, 8 | 16)
                    || matches!(h.color, 0 | 3) && matches!(h.depth, 1 | 2 | 4))
                    || h.color == 3 && h.depth == 16
                    || h.compression != 0
                    || h.filter != 0
                {
                    return Err(Error::Header);
                }
                let old = self.header;
                self.header = h;
                self.channels = channels;
                let pass = usize::from(h.interlace != 0);
                if let Err(e) = need(b.ring.len(), self.ring_size(0)?, Buffer::Ring)
                    .and_then(|()| self.set_pass(pass, b))
                {
                    self.header = old;
                    self.channels = 0;
                    return Err(e);
                }
                Ok(13)
            }
            b"IDAT" => {
                if input.is_empty() {
                    return Ok(0);
                }
                let p = inflate.decompress(
                    input,
                    &mut b.dictionary[..inflate::DICTIONARY_SIZE],
                    self.out,
                    inflate::DICTIONARY_SIZE - self.out,
                    inflate::PARSE_ZLIB_HEADER | inflate::HAS_MORE_INPUT,
                );
                if (p.status as i32) < 0 {
                    return Err(Error::Inflate);
                }
                self.out += p.produced;
                if p.status == inflate::Status::Done || self.out == inflate::DICTIONARY_SIZE {
                    for i in 0..self.out {
                        let byte = b.dictionary[i];
                        self.data_byte(byte, b, cb)?;
                    }
                    self.out = 0;
                }
                Ok(p.consumed)
            }
            b"PLTE" => {
                if input.len() < 3 {
                    return Ok(0);
                }
                b.palette[self.palettes * 3..self.palettes * 3 + 3].copy_from_slice(&input[..3]);
                self.palettes += 1;
                Ok(3)
            }
            b"tRNS" => {
                let n = match self.header.color {
                    0 => 2,
                    2 => 6,
                    3 => 1,
                    _ => return Err(Error::Chunk),
                };
                if input.len() < n {
                    return Ok(0);
                }
                b.transparency[self.trans..self.trans + n].copy_from_slice(&input[..n]);
                self.trans += 1;
                Ok(n)
            }
            b"gAMA" => {
                if input.len() < 4 {
                    return Ok(0);
                }
                let gamma = u32be(input);
                if self.display_gamma > 0.0 && gamma != 0 {
                    if self.channels == 0 {
                        return Err(Error::Order);
                    }
                    let max = self.maxval();
                    need(b.gamma.len(), max as usize + 1, Buffer::Gamma)?;
                    for i in 0..=max {
                        b.gamma[i as usize] = (math.pow(
                            f64::from(i) / f64::from(max),
                            100000.0 / f64::from(gamma) / self.display_gamma,
                        ) * 255.0
                            + 0.5) as u8;
                    }
                    self.gamma_len = max as usize + 1;
                } else {
                    self.gamma_len = 0;
                }
                Ok(4)
            }
            // The pinned C enum spells this 0x424b4744 (uppercase B), so the
            // standard lowercase bKGD remains an unknown, ignored chunk.
            b"BKGD" => {
                let n = match self.header.color {
                    3 => 1,
                    0 | 4 => 2,
                    2 | 6 => 6,
                    _ => return Err(Error::Chunk),
                };
                // Native stalls on incomplete input before touching the unsafe path.
                if input.len() < n {
                    return Ok(0);
                }
                if self.channels == 0 {
                    return Err(Error::Order);
                }
                // Native writes six values into v[4] for the uppercase RGB/RGBA path.
                if matches!(self.header.color, 2 | 6) {
                    return Err(Error::NativeUndefinedBackground);
                }
                let mut v = [0; 4];
                let mut ix = 0;
                let mut bits = 0;
                // The original reads modulo chunk input length (including malformed depth cases).
                for value in v.iter_mut().take(n) {
                    *value = match self.header.depth {
                        1 | 2 | 4 => {
                            if bits >= 8 {
                                bits = 0;
                                ix = (ix + 1) % input.len();
                            }
                            bits += self.header.depth;
                            u16::from((input[ix] >> (8 - bits)) & ((1 << self.header.depth) - 1))
                        }
                        8 => {
                            let a = input[ix];
                            ix = (ix + 1) % input.len();
                            u16::from(a)
                        }
                        _ => {
                            let a = input[ix];
                            ix = (ix + 1) % input.len();
                            let c = input[ix];
                            ix = (ix + 1) % input.len();
                            u16::from_be_bytes([a, c])
                        }
                    };
                }
                let rgba = self.color(v, b)?;
                self.background.copy_from_slice(&rgba[..3]);
                Ok(n)
            }
            b"IEND" => Ok(0),
            _ => Ok(input.len()),
        }
    }
    /// Same partial-consumption contract as pngle_feed: retain the unconsumed tail.
    /// Capacity admissions do not poison the parser; enlarge the named borrowed buffer and retry.
    pub fn feed(
        &mut self,
        input: &[u8],
        b: &mut Buffers<'_>,
        inflate: &mut State,
        math: &dyn Math,
        cb: &mut dyn Callbacks,
    ) -> Result<usize, Error> {
        if self.phase == Phase::Failed {
            return Err(self.error.unwrap_or(Error::Header));
        }
        // A caller may reborrow a larger buffer after admission. Validate all live
        // domains on every call, before any indexing or coroutine mutation.
        let ring_needed = if self.channels == 0 {
            0
        } else {
            self.ring_size(0)?
        };
        need(b.ring.len(), ring_needed, Buffer::Ring)?;
        need(b.palette.len(), self.palette_size, Buffer::Palette)?;
        need(b.transparency.len(), self.trans_size, Buffer::Transparency)?;
        need(b.gamma.len(), self.gamma_len, Buffer::Gamma)?;
        if self.started {
            need(
                b.dictionary.len(),
                inflate::DICTIONARY_SIZE,
                Buffer::Dictionary,
            )?;
        }
        let result = self.feed_inner(input, b, inflate, math, cb);
        if let Err(e) = result {
            if !matches!(e, Error::Capacity { .. }) {
                self.phase = Phase::Failed;
                self.error = Some(e);
            }
        }
        result
    }
    fn feed_inner(
        &mut self,
        input: &[u8],
        b: &mut Buffers<'_>,
        inflate: &mut State,
        math: &dyn Math,
        cb: &mut dyn Callbacks,
    ) -> Result<usize, Error> {
        let mut pos = 0;
        while pos < input.len() {
            let tail = &input[pos..];
            let old = self.phase;
            let n = match self.phase {
                Phase::Failed => return Err(self.error.unwrap_or(Error::Header)),
                Phase::Eof => tail.len(),
                Phase::Initial => {
                    if tail.len() < 8 {
                        0
                    } else {
                        if tail[..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
                            return Err(Error::Signature);
                        }
                        self.phase = Phase::Header;
                        8
                    }
                }
                Phase::Header => {
                    if tail.len() < 8 {
                        0
                    } else {
                        let len = u32be(tail);
                        let kind = [tail[4], tail[5], tail[6], tail[7]];
                        self.chunk_start(kind, len, b, cb)
                            .map_err(|e| admission_at(e, pos))?;
                        self.kind = kind;
                        self.remain = len;
                        self.crc = crc(!0, &kind);
                        self.phase = Phase::Body;
                        8
                    }
                }
                Phase::Body => {
                    let end = tail.len().min(self.remain as usize);
                    let n = self
                        .body(&tail[..end], b, inflate, math, cb)
                        .map_err(|e| admission_at(e, pos))?;
                    if n > end {
                        return Err(Error::Chunk);
                    }
                    self.remain -= n as u32;
                    self.crc = crc(self.crc, &tail[..n]);
                    if self.remain == 0 {
                        self.phase = Phase::Crc;
                    }
                    n
                }
                Phase::Crc => {
                    if tail.len() < 4 {
                        0
                    } else {
                        if u32be(tail) != !self.crc {
                            return Err(Error::Crc);
                        }
                        self.phase = if self.kind == *b"IEND" {
                            cb.done()?;
                            Phase::Eof
                        } else {
                            Phase::Header
                        };
                        4
                    }
                }
            };
            pos += n;
            if n == 0 && old == self.phase {
                break;
            }
        }
        Ok(pos)
    }
}
