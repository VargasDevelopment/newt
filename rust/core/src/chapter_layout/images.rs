#![forbid(unsafe_code)]
use crate::member_check::{self, Checked};
use super::{text::local, types::Images};
use crate::{
    xml_input,
    xml_session::{Attributes, Handler},
    zip_container::{Entry, InflateScratch, Reader, Status},
    zip_window::{arena::Arena, source::BookSource},
};
use crate::image_probe::{self, ImageInfo, Kind};
#[derive(Clone, Copy, Default)]
pub struct Probed {
    pub hash: u32,
    pub info: ImageInfo,
}
/// Newt: lookups return checked members, so no image read skips the open-time
/// check. `source` is the image service's own read handle of the same book.
pub trait Catalog {
    fn member(&self, source: &mut dyn BookSource, path: &[u8]) -> Result<Checked, Status>;
    fn member_by_hash(&self, source: &mut dyn BookSource, hash: u32) -> Result<Checked, Status>;
}
/// One borrowed decoder is reborrowed for each sequential image probe. During
/// pre-scan this is the temporary large decoder; resident layout can use another.
pub struct BookImages<'a> {
    pub source: &'a mut dyn BookSource,
    pub catalog: Option<&'a dyn Catalog>,
    pub directory: &'a [u8],
    pub decoder: Option<InflateScratch<'a>>,
    pub skip: &'a mut [u8],
    pub path_scratch: &'a mut [u8],
    pub table: &'a [Probed],
}
impl BookImages<'_> {
    fn probe_entry(&mut self, e: &Checked) -> ImageInfo {
        let mut info = ImageInfo::default();
        let decoder = self.decoder.as_mut().map(|d| InflateScratch {
            state: &mut *d.state,
            dictionary: &mut *d.dictionary,
            input: &mut *d.input,
        });
        let _ = image_probe::probe(self.source, e, decoder, self.skip, &mut info);
        info
    }
}
impl Images for BookImages<'_> {
    fn resolve(&mut self, src: &[u8], path: &mut [u8], frag: &mut [u8]) -> Option<(usize, usize)> {
        let mut a = Arena::new(self.path_scratch);
        let mut f = None;
        let r = xml_input::resolve_href(
            &mut a,
            Some(self.directory),
            Some(src),
            if frag.is_empty() { None } else { Some(&mut f) },
        )?;
        let p = xml_input::c_bytes(a.view(r)?);
        if p.len() > path.len() {
            return None;
        }
        path[..p.len()].copy_from_slice(p);
        let n = p.len();
        let mut nf = 0;
        if let Some(f) = f {
            let f = xml_input::c_bytes(a.view(f)?);
            nf = f.len().min(frag.len());
            frag[..nf].copy_from_slice(&f[..nf]);
        }
        Some((n, nf))
    }
    fn probe(&mut self, path: &[u8]) -> Option<(u32, u32)> {
        let catalog = self.catalog?;
        let hash = crate::css::hash(path);
        let info = if let Some(p) = self.table.iter().find(|p| p.hash == hash) {
            p.info
        } else {
            // A damaged image gives no size, like a missing one.
            let e = catalog.member(&mut *self.source, path).ok()?;
            self.probe_entry(&e)
        };
        if info.kind == Kind::Unknown || info.width == 0 || info.height == 0 {
            None
        } else {
            Some((u32::from(info.width), u32::from(info.height)))
        }
    }
}
/// Raw image detector retains exactly two bytes across reads. Reader-open and
/// buffer failure are treated conservatively by the caller as "mentions".
pub fn mentions<S: BookSource + ?Sized>(reader: &mut Reader<'_, S>, buf: &mut [u8; 2048]) -> bool {
    let mut carry = [0; 2];
    loop {
        let n = reader.read(buf);
        if n <= 0 {
            return false;
        }
        let n = n as usize;
        for i in 0..n {
            let c2 = if i >= 2 { buf[i - 2] } else { carry[i] };
            let c1 = if i >= 1 { buf[i - 1] } else { carry[i + 1] };
            if c1 == b'i' && buf[i] == b'm' && matches!(c2, b'<' | b':') {
                return true;
            }
        }
        if n >= 2 {
            carry.copy_from_slice(&buf[n - 2..n])
        } else {
            carry[0] = carry[1];
            carry[1] = buf[0]
        }
    }
}
/// Collector consumes the first local src (or href if absent). Its storage is
/// independent of layout and is released before decoder-heavy dimension probes.
pub struct Collector<'a> {
    pub images: &'a mut dyn Images,
    pub table: &'a mut [Probed],
    pub count: usize,
    pub stopped: bool,
}
impl Handler for Collector<'_> {
    fn start_element(&mut self, n: &[u8], atts: &mut dyn Attributes) {
        if !matches!(local(n), b"img" | b"image") || self.stopped {
            return;
        }
        let Some(src) = atts.local(b"src").or_else(|| atts.local(b"href")) else {
            return;
        };
        let mut path = [0; 512];
        let mut frag = [];
        if let Some((n, _)) = self.images.resolve(src, &mut path, &mut frag) {
            let hash = crate::css::hash(&path[..n]);
            if !self.table[..self.count].iter().any(|p| p.hash == hash)
                && self.count < self.table.len()
            {
                self.table[self.count] = Probed {
                    hash,
                    info: ImageInfo::default(),
                };
                self.count += 1;
                if self.count >= self.table.len() {
                    self.stopped = true
                }
            }
        }
    }
    fn stop_parse(&self) -> bool {
        self.stopped
    }
}
pub fn probe_collected(images: &mut BookImages<'_>, table: &mut [Probed]) {
    let Some(catalog) = images.catalog else {
        return;
    };
    for p in table {
        if let Ok(e) = catalog.member_by_hash(&mut *images.source, p.hash) {
            p.info = images.probe_entry(&e)
        }
    }
}
/// Preserve partial collector success after parse failure, like prescanImages.
pub fn keep_prescan(parse_status: Status, count: usize) -> bool {
    parse_status == Status::Ok || count > 0
}
/// Full pre-scan orchestration. Chapter and book read/decoder domains are
/// disjoint; the collector/parser is closed before book probes begin.
pub fn prescan<
    S: BookSource + ?Sized,
    B: crate::xml_session::ParserBackend + ?Sized,
    P: crate::xml_session::SessionPolicy<B>,
>(
    chapter: &mut S,
    entry: &Checked,
    backend: &mut B,
    mut chapter_decoder: Option<InflateScratch<'_>>,
    buffer: &mut [u8; 2048],
    filter: &mut crate::xml_input::EntityFilter,
    images: &mut BookImages<'_>,
    table: &mut [Probed],
    policy: P,
) -> usize {
    if images.catalog.is_none() || table.is_empty() {
        return 0;
    }
    let mention = {
        let dec = chapter_decoder.as_mut().map(|d| InflateScratch {
            state: &mut *d.state,
            dictionary: &mut *d.dictionary,
            input: &mut *d.input,
        });
        let mut reader = Reader::new(chapter, dec);
        reader.open(entry) != Status::Ok || mentions(&mut reader, buffer)
    };
    if !mention {
        return 0;
    }
    let mut c = Collector {
        images,
        table,
        count: 0,
        stopped: false,
    };
    let dec = chapter_decoder.as_mut().map(|d| InflateScratch {
        state: &mut *d.state,
        dictionary: &mut *d.dictionary,
        input: &mut *d.input,
    });
    let mut stream = crate::xml_session::XmlStream::with_policy(chapter, dec, backend, policy);
    let mut st = stream.open(
        entry,
        crate::xml_session::Resources {
            buffer: Some(buffer),
            filter: Some(filter),
        },
        true,
    );
    if st == Status::Ok {
        let mut end = false;
        while !end && !c.stopped {
            st = stream.feed_chunk(&mut c, Some(&mut end));
            if st != Status::Ok {
                break;
            }
        }
    }
    stream.close();
    drop(stream);
    let count = c.count;
    drop(c);
    if keep_prescan(st, count) {
        probe_collected(images, &mut table[..count]);
        count
    } else {
        0
    }
}
/// Catalog lookup adapter only. Accepted Catalog owns its source borrow; the
/// image service needs a separate read handle/view of the same immutable book.
/// No pointer escape or second mutable borrow of the catalog's source occurs.
impl<S: BookSource + ?Sized> Catalog for crate::zip_container::Catalog<'_, '_, S> {
    fn member(&self, source: &mut dyn BookSource, path: &[u8]) -> Result<Checked, Status> {
        let entry = crate::zip_container::Catalog::find(self, Some(path)).ok_or(Status::NotFound)?;
        self.checked(source, entry)
    }
    fn member_by_hash(&self, source: &mut dyn BookSource, hash: u32) -> Result<Checked, Status> {
        let entry = self.find_by_hash(hash).ok_or(Status::NotFound)?;
        self.checked(source, entry)
    }
}
impl<S: BookSource + ?Sized> crate::zip_container::Catalog<'_, '_, S> {
    fn checked(&self, source: &mut dyn BookSource, entry: &Entry) -> Result<Checked, Status> {
        let name = self.name(entry).ok_or(Status::Damaged)?;
        member_check::check(source, entry, name, self.directory_offset()).map(|(member, _)| member)
    }
}
