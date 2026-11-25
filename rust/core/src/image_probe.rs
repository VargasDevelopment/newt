// Copyright (c) 2026 FreeInk. MIT; see rust/THIRD_PARTY.md.
//! Exact pinned ImageProbe algorithm; caller-owned decoder and skip storage.
#![forbid(unsafe_code)]
use crate::member_check::Checked;
use crate::zip_container::{InflateScratch, Reader, Status};
use crate::zip_window::source::BookSource;

pub const SKIP_BYTES: usize = 1024;
const MAX_JPEG_SCAN: u32 = 128 * 1024;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Kind {
    #[default]
    Unknown,
    Png,
    Jpeg,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImageInfo {
    pub kind: Kind,
    pub width: u16,
    pub height: u16,
    pub progressive: bool,
}
fn be16(p: &[u8]) -> u16 {
    u16::from_be_bytes([p[0], p[1]])
}
fn be32(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}
struct Stream<'a, 'reader, S: BookSource + ?Sized> {
    reader: &'a mut Reader<'reader, S>,
    consumed: u32,
}
impl<S: BookSource + ?Sized> Stream<'_, '_, S> {
    fn exact(&mut self, dst: &mut [u8]) -> bool {
        let mut got = 0;
        while got < dst.len() {
            let n = self.reader.read(&mut dst[got..]);
            if n <= 0 {
                return false;
            }
            got += n as usize;
        }
        // An incomplete exact read contributes no consumed bytes.
        self.consumed += dst.len() as u32;
        true
    }
    fn skip(&mut self, mut len: usize, buf: &mut [u8]) -> bool {
        while len > 0 {
            let want = len.min(SKIP_BYTES);
            let n = self.reader.read(&mut buf[..want]);
            if n <= 0 {
                return false;
            }
            len -= n as usize;
            self.consumed += n as u32;
        }
        true
    }
}
struct Replay<'a> {
    bytes: &'a [u8],
}
impl Replay<'_> {
    fn next<S: BookSource + ?Sized>(
        &mut self,
        stream: &mut Stream<'_, '_, S>,
        dst: &mut [u8],
    ) -> bool {
        let take = self.bytes.len().min(dst.len());
        dst[..take].copy_from_slice(&self.bytes[..take]);
        self.bytes = &self.bytes[take..];
        if take == dst.len() {
            true
        } else {
            stream.exact(&mut dst[take..])
        }
    }
    fn skip<S: BookSource + ?Sized>(
        &mut self,
        stream: &mut Stream<'_, '_, S>,
        len: usize,
        buf: &mut [u8],
    ) -> bool {
        let take = self.bytes.len().min(len);
        self.bytes = &self.bytes[take..];
        stream.skip(len - take, buf)
    }
}
/// Reset output before open; open failures precede missing skip storage.
/// Decoder storage is the accepted typed ZIP domain, separate from staging.
/// A staging slice shorter than 1024 is OOM; extra bytes are never used.
/// Neither domain is allocated or retained here. Failure to read the initial
/// 26 bytes, or a later segment, deliberately returns Ok/Unknown.
pub fn probe<S: BookSource + ?Sized>(
    source: &mut S,
    entry: &Checked,
    decode: Option<InflateScratch<'_>>,
    staging: &mut [u8],
    out: &mut ImageInfo,
) -> Status {
    *out = ImageInfo::default();
    let mut reader = Reader::new(source, decode);
    let status = reader.open(entry);
    if status != Status::Ok {
        return status;
    }
    if staging.len() < SKIP_BYTES {
        return Status::OutOfMemory;
    }
    let mut stream = Stream {
        reader: &mut reader,
        consumed: 0,
    };
    let mut hdr = [0; 26];
    if !stream.exact(&mut hdr) {
        return Status::Ok;
    }
    if &hdr[..8] == b"\x89PNG\r\n\x1a\n" && &hdr[12..16] == b"IHDR" {
        let w = be32(&hdr[16..20]);
        let h = be32(&hdr[20..24]);
        if w > 0 && h > 0 && w <= 0xffff && h <= 0xffff {
            out.kind = Kind::Png;
            out.width = w as u16;
            out.height = h as u16;
        }
        return Status::Ok;
    }
    if hdr[0] == 0xff && hdr[1] == 0xd8 {
        let mut replay = Replay { bytes: &hdr[2..] };
        let mut seg = [0; 2];
        while stream.consumed < MAX_JPEG_SCAN {
            if !replay.next(&mut stream, &mut seg) {
                break;
            }
            if seg[0] != 0xff {
                break;
            }
            let marker = seg[1];
            if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
                continue;
            }
            if marker == 0xd9 || marker == 0xda {
                break;
            }
            if !replay.next(&mut stream, &mut seg) {
                break;
            }
            let len = be16(&seg);
            if len < 2 {
                break;
            }
            let is_sof = (0xc0..=0xcf).contains(&marker)
                && marker != 0xc4
                && marker != 0xc8
                && marker != 0xcc;
            if is_sof {
                let mut sof = [0; 5];
                if replay.next(&mut stream, &mut sof) {
                    out.kind = Kind::Jpeg;
                    out.height = be16(&sof[1..3]);
                    out.width = be16(&sof[3..5]);
                    out.progressive = marker == 0xc2;
                }
                break;
            }
            if !replay.skip(&mut stream, usize::from(len) - 2, staging) {
                break;
            }
        }
    }
    Status::Ok
}
