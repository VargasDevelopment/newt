//! EPUB admission checks. Typed resources are caller-owned; no hidden inflater stack.
use crate::member_check::Checked;
use crate::zip_container::{self as zip, Directory, Entry, InflateScratch, Reader, Status};
use crate::zip_record_rules::*;
use crate::zip_window::{arena::Arena, source::BookSource, window::ZipDirectoryWindow};
pub const EXTERNAL_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const SOURCE_READ_BYTES: usize = 32768;
pub const INFLATE_SLACK: u64 = 65536;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkPhase {
    Idle,
    Admission,
    Metadata,
    Layout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WorkKind {
    Phase,
    InspectBegin,
    InspectEnd,
    CatalogBegin,
    CatalogEnd,
    CompareBegin,
    CompareEnd,
    MemberBegin,
    StreamBegin,
    StreamEnd,
    MemberEnd,
    ActionBoundary,
    OpenRoute,
    CacheMiss,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkEvent {
    pub kind: WorkKind,
    pub phase: WorkPhase,
    pub member: u16,
    pub compressed_bytes: u32,
    pub expanded_bytes: u32,
    pub result: i32,
    pub method: u8,
    pub category: u8,
}
impl WorkEvent {
    pub fn new(kind: WorkKind, result: i32) -> Self {
        Self {
            kind,
            phase: WorkPhase::Idle,
            member: 0,
            compressed_bytes: 0,
            expanded_bytes: 0,
            result,
            method: 0,
            category: 0,
        }
    }
}
pub type Observer = Option<fn(WorkEvent)>;
/// Synchronous work callback borrowing the UI task's existing Scheduler.
pub type Yield<'a> = Option<&'a dyn WorkYield>;
#[path = "admission_work.rs"]
mod work;
pub use work::{ScopedWork, WorkYield};
pub use work::{ActionScope, action_boundary, open_route, cache_miss};
/// Read-only phase observation. Callers must not nest scopes or reenter parsing.
pub struct WorkScope<'a> {
    observer: Observer,
    on_yield: Yield<'a>,
}
impl<'a> WorkScope<'a> {
    pub fn new(observer: Observer, phase: WorkPhase) -> Self {
        Self::new_scoped(None, observer, phase)
    }
    pub fn new_scoped(on_yield: Yield<'a>, observer: Observer, phase: WorkPhase) -> Self {
        let mut event = WorkEvent::new(WorkKind::Phase, 0);
        event.phase = phase;
        observe(on_yield, observer, event);
        Self { observer, on_yield }
    }
}
impl Drop for WorkScope<'_> {
    fn drop(&mut self) {
        emit_to(self.on_yield, self.observer, WorkKind::Phase, 0);
    }
}

pub fn emit(observer: Observer, kind: WorkKind, result: i32) {
    emit_to(None, observer, kind, result);
}
pub fn emit_to(on_yield: Yield<'_>, observer: Observer, kind: WorkKind, result: i32) {
    observe(on_yield, observer, WorkEvent::new(kind, result));
}
pub fn observe(on_yield: Yield<'_>, observer: Observer, event: WorkEvent) {
    if let Some(hook) = on_yield { hook.observe(event); }
    if let Some(f) = observer {
        f(event);
    }
}
/// Optional attempt policy is shared with XML; ordinary callers pass None.
/// Yield may schedule but must not change source content or binding.
pub struct ScheduledSource<'a, S: BookSource + ?Sized> {
    pub source: &'a mut S,
    pub on_yield: Yield<'a>,
    pub policy: Option<&'a dyn crate::xml_session::SourcePolicy>,
}
impl<'a, S: BookSource + ?Sized> ScheduledSource<'a, S> {
    pub fn new(source: &'a mut S, on_yield: Yield<'a>,
        policy: Option<&'a dyn crate::xml_session::SourcePolicy>) -> Self {
        Self { source, on_yield, policy }
    }
}
impl<S: BookSource + ?Sized> BookSource for ScheduledSource<'_, S> {
    fn size(&self) -> u64 {
        self.source.size()
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if self.policy.is_some_and(|policy| policy.cancelled()) { return -1; }
        let want = out.len().min(SOURCE_READ_BYTES);
        let got = self.source.read_at(at, &mut out[..want]);
        if got < 0 || got as usize > want || (got == 0 && want != 0 && at < self.source.size()) {
            if let Some(policy) = self.policy { policy.result(false); }
            return -1;
        }
        if got > 0 {
            if let Some(f) = self.on_yield {
                f.yield_now();
            }
        }
        if self.policy.is_some_and(|policy| policy.cancelled()) { return -1; }
        got
    }
}
fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn read<S: BookSource + ?Sized>(s: &mut S, mut at: u64, mut out: &mut [u8]) -> bool {
    while !out.is_empty() {
        let got = s.read_at(at, out);
        if got <= 0 || got as usize > out.len() {
            return false;
        }
        at += got as u64;
        out = &mut out[got as usize..];
    }
    true
}
fn window_yield<S: BookSource + ?Sized>(s: &mut ScheduledSource<'_, S>) {
    if let Some(f) = s.on_yield {
        f.yield_now();
    }
}
/// Every central record, in original precedence order. Local headers are
/// checked when each member opens (`member_check`), so a damaged member
/// fails where it is read instead of refusing the whole book.
pub fn inspect<S: BookSource + ?Sized>(
    source: &mut ScheduledSource<'_, S>,
    parse: &mut Arena<'_>,
    scan: &mut [u8; 4099],
) -> Status {
    let mut d = Directory::default();
    let status = zip::locate(source, scan, &mut d);
    if status != Status::Ok {
        return status;
    }
    if d.entries == 0 || d.entries > 1024 || d.size > 1024 * 1024 {
        return Status::Unsupported;
    }
    let end = u64::from(d.offset) + u64::from(d.size);
    let mut h = [0; 46];
    if !read(source, end, &mut h[..22]) {
        return Status::IoError;
    }
    if le32(&h, 0) != 0x06054b50
        || le16(&h, 4) != 0
        || le16(&h, 6) != 0
        || le16(&h, 8) != d.entries
        || le16(&h, 10) != d.entries
        || end + 22 + u64::from(le16(&h, 20)) != source.size()
    {
        return Status::Unsupported;
    }
    let hook = if source.on_yield.is_some() {
        Some(window_yield::<S> as fn(&mut ScheduledSource<'_, S>))
    } else {
        None
    };
    let mut central = ZipDirectoryWindow::new(source, parse, u64::from(d.offset), end, hook);
    let mut at = u64::from(d.offset);
    let mut expanded = 0u64;
    for _ in 0..d.entries {
        if at > end || end - at < 46 {
            return Status::Truncated;
        }
        if !central.read(at, Some(&mut h), 46) {
            return Status::IoError;
        }
        if le32(&h, 0) != 0x02014b50 {
            return Status::NotZip;
        }
        let (flags, method, name) = (le16(&h, 8), le16(&h, 10), le16(&h, 28));
        let (compressed, uncompressed) = (le32(&h, 20), le32(&h, 24));
        if zip_central_record_rule(0, method, name, uncompressed)
            == ZipCentralRecordRule::Unsupported
            || u64::from(compressed) > EXTERNAL_MAX_BYTES
        {
            return Status::Unsupported;
        }
        expanded += u64::from(uncompressed);
        if expanded > EPUB_MAX_EXPANDED_BYTES {
            return Status::Unsupported;
        }
        let local = u64::from(le32(&h, 42));
        let record = 46 + u64::from(name) + u64::from(le16(&h, 30)) + u64::from(le16(&h, 32));
        if record > end - at || local + 30 > u64::from(d.offset) {
            return Status::Truncated;
        }
        match zip_member_flag_rule(flags) {
            ZipCentralRecordRule::Encrypted => return Status::Encrypted,
            ZipCentralRecordRule::Unsupported => return Status::Unsupported,
            ZipCentralRecordRule::Ok => (),
        }
        let mut n = 0u64;
        while n < u64::from(name) {
            let mut a = [0; 64];
            let take = (u64::from(name) - n).min(64) as usize;
            if !central.read(at + 46 + n, Some(&mut a), take as u32) {
                return Status::IoError;
            }
            if a[..take].contains(&0) {
                return Status::Unsupported;
            }
            n += take as u64;
        }
        at += record;
    }
    if at == end {
        Status::Ok
    } else {
        Status::Unsupported
    }
}
pub fn inflate_input_bounded(e: &Entry) -> bool {
    e.method != 8
        || u64::from(e.compressed_size)
            <= u64::from(e.uncompressed_size) + u64::from(e.uncompressed_size) / 8 + INFLATE_SLACK
}
pub fn category(name: &[u8]) -> u8 {
    let Some(dot) = name.iter().rposition(|b| *b == b'.') else {
        return 0;
    };
    let ext = &name[dot..];
    if [b".jpg".as_slice(), b".jpeg", b".png", b".gif", b".svg"]
        .iter()
        .any(|v| ext.eq_ignore_ascii_case(v))
    {
        1
    } else if [b".xhtml".as_slice(), b".html", b".xml", b".opf"]
        .iter()
        .any(|v| ext.eq_ignore_ascii_case(v))
    {
        2
    } else {
        0
    }
}
/// Scratch presence is the typed-domain OOM decision, independent of byte arena.
pub fn expand<S: BookSource + ?Sized>(
    source: &mut S,
    entry: &Entry,
    index: u16,
    name: &[u8],
    decoder: Option<InflateScratch<'_>>,
    chunk: Option<&mut [u8; 2048]>,
    on_yield: crate::admission::Yield<'_>,
    observer: Observer,
) -> Status {
    member_events(entry, index, name, on_yield, observer,
        || stream(source, entry, decoder, chunk, on_yield, observer))
}
/// Admission's required member: the open-time check runs inside the member's
/// MemberBegin/MemberEnd pair, so a damaged member's failure names its stage.
pub fn check_and_expand<S: BookSource + ?Sized>(
    source: &mut S,
    entry: &Entry,
    index: u16,
    name: &[u8],
    directory_offset: u32,
    decoder: Option<InflateScratch<'_>>,
    chunk: Option<&mut [u8; 2048]>,
    on_yield: crate::admission::Yield<'_>,
    observer: Observer,
) -> Result<Checked, Status> {
    let mut checked = None;
    let status = member_events(entry, index, name, on_yield, observer, || {
        match crate::member_check::check(source, entry, name, directory_offset) {
            Ok((member, _)) => stream(source, checked.insert(member), decoder, chunk, on_yield, observer),
            Err(status) => status,
        }
    });
    match (status, checked) {
        (Status::Ok, Some(member)) => Ok(member),
        _ => Err(status),
    }
}
fn member_events(
    entry: &Entry,
    index: u16,
    name: &[u8],
    on_yield: crate::admission::Yield<'_>,
    observer: Observer,
    body: impl FnOnce() -> Status,
) -> Status {
    let mut event = WorkEvent::new(WorkKind::MemberBegin, 0);
    event.member = index;
    event.method = entry.method as u8;
    event.category = category(name);
    event.compressed_bytes = entry.compressed_size;
    event.expanded_bytes = entry.uncompressed_size;
    observe(on_yield, observer, event);
    let result = body();
    event.kind = WorkKind::MemberEnd;
    event.result = result as i32;
    observe(on_yield, observer, event);
    result
}
fn stream<S: BookSource + ?Sized>(
    source: &mut S,
    entry: &Entry,
    decoder: Option<InflateScratch<'_>>,
    chunk: Option<&mut [u8; 2048]>,
    on_yield: crate::admission::Yield<'_>,
    observer: Observer,
) -> Status {
    {
        if !inflate_input_bounded(entry) {
            return Status::Unsupported;
        }
        let Some(chunk) = chunk else {
            return Status::OutOfMemory;
        };
        let mut reader = Reader::new(source, decoder);
        let status = reader.open(entry);
        if status != Status::Ok {
            return status;
        }
        let mut produced = 0u64;
        loop {
            let want = (u64::from(entry.uncompressed_size) - produced + 1).min(2048) as usize;
            emit_to(on_yield, observer, WorkKind::StreamBegin, 0);
            let got = reader.read(&mut chunk[..want]);
            emit_to(on_yield, observer, WorkKind::StreamEnd, got);
            if got < 0 || got as usize > want {
                return Status::IoError;
            }
            if got == 0 {
                return if produced == u64::from(entry.uncompressed_size) {
                    Status::Ok
                } else {
                    Status::Truncated
                };
            }
            produced += got as u64;
            if produced > u64::from(entry.uncompressed_size) {
                return Status::Unsupported;
            }
            if let Some(f) = on_yield {
                f.yield_now();
            }
        }
    }
}
/// A sequential chapter adapter. Successful construction corresponds to open Ok.
/// On failed open no source object is published (stronger safe API boundary).
pub struct ChapterSource<'a, S: BookSource + ?Sized> {
    reader: Reader<'a, S>,
    length: u32,
    consumed: u32,
    on_yield: Yield<'a>,
    policy: Option<&'a dyn crate::xml_session::SourcePolicy>,
}
impl<'a, S: BookSource + ?Sized> ChapterSource<'a, S> {
    pub fn open(
        source: &'a mut S,
        entry: &'a Entry,
        decoder: Option<InflateScratch<'a>>,
        on_yield: Yield<'a>,
        policy: Option<&'a dyn crate::xml_session::SourcePolicy>,
    ) -> Result<Self, Status> {
        if !inflate_input_bounded(entry) {
            return Err(Status::Unsupported);
        }
        let mut reader = Reader::new(source, decoder);
        let status = reader.open(entry);
        if status != Status::Ok {
            return Err(status);
        }
        Ok(Self {
            reader,
            length: entry.uncompressed_size,
            consumed: 0,
            on_yield,
            policy,
        })
    }
    /// Guard probes use the original source, never this decoded byte stream.
    pub fn with_source<R>(&mut self, operation: impl FnOnce(&mut S) -> R) -> R {
        self.reader.with_source(operation)
    }
    pub fn finish(&mut self) -> bool {
        if self.policy.is_some_and(|policy| policy.cancelled()) { return false; }
        let mut extra = [0];
        let ended = self.consumed == self.length && self.reader.read(&mut extra) == 0;
        if let Some(policy) = self.policy {
            policy.result(ended);
            if policy.cancelled() { return false; }
        }
        ended
    }
}
impl<S: BookSource + ?Sized> BookSource for ChapterSource<'_, S> {
    fn size(&self) -> u64 {
        u64::from(self.length)
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if self.policy.is_some_and(|policy| policy.cancelled()) { return -1; }
        if at != u64::from(self.consumed) || self.consumed > self.length {
            return -1;
        }
        let want = out
            .len()
            .min(2048)
            .min((self.length - self.consumed) as usize);
        if want == 0 {
            return 0;
        }
        let got = self.reader.read(&mut out[..want]);
        if got <= 0 || got as usize > want {
            if let Some(policy) = self.policy { policy.result(false); }
            return -1;
        }
        self.consumed += got as u32;
        if let Some(f) = self.on_yield {
            f.yield_now();
        }
        if self.policy.is_some_and(|policy| policy.cancelled()) { return -1; }
        got
    }
}
/// Admission consumes all names before the caller resets its arenas.
/// Reset is unconditional, including catalog/open/required-member failures.
pub fn admit<S: BookSource + ?Sized, B: crate::xml_session::ParserBackend + ?Sized>(
    source: &mut S,
    catalog: &mut Arena<'_>,
    parse: &mut Arena<'_>,
    resources: &mut crate::canonical::Resources<'_, B>,
    on_yield: crate::admission::Yield<'_>,
    observer: Observer,
) -> Status {
    let mut scheduled = ScheduledSource::new(source, on_yield, resources.policy);
    emit_to(on_yield, observer, WorkKind::InspectBegin, 0);
    let mut status = inspect(&mut scheduled, parse, resources.scan);
    emit_to(on_yield, observer, WorkKind::InspectEnd, status as i32);
    if status == Status::Ok {
        parse.reset();
        status = match crate::canonical::open(
            &mut scheduled,
            catalog,
            resources,
            true,
            on_yield,
            observer,
        ) {
            Ok(book) => book.validate_selection(on_yield, observer),
            Err(e) => e,
        };
    }
    catalog.reset();
    parse.reset();
    status
}
