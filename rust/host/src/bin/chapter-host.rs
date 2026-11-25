//! Host transport for source-only packet; native XML and linebreak are explicit.
#[path="../xml_session/expat.rs"] mod expat;
#[path="../chapter_layout/linebreak.rs"] mod linebreak;
use newt_core::{
    css::{CssRule, CssStylesheetBuilder, TextAlign},
    chapter_layout::engine::Engine,
    chapter_layout::session::{ChapterSession, LayoutHandler},
    chapter_layout::types::*,
    xml_input::EntityFilter,
    xml_session::{Resources, XmlStream},
    member_check::Checked,
    zip_container::Status,
    zip_window::source::BookSource,
};
use std::io::{self, Read};
fn hex(b: &[u8]) -> String {
    if b.is_empty() {
        return "-".into();
    }
    b.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode(s: &str) -> Vec<u8> {
    if s == "-" {
        return vec![];
    }
    assert!(
        !s.is_empty()
            && s.len() % 2 == 0
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    s.as_bytes()
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn num(s: &str) -> u32 {
    assert!(!s.is_empty() && (s == "0" || !s.starts_with('0')) && s.bytes().all(|b| b.is_ascii_digit()));
    s.parse().unwrap()
}
struct Source {
    bytes: Vec<u8>,
    max: usize,
    fail: i64,
    calls: i64,
    book: bool,
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let call = self.calls;
        self.calls += 1;
        let n = if call == self.fail {
            -1
        } else {
            let take = out
                .len()
                .min(self.max)
                .min(self.bytes.len().saturating_sub(at as usize));
            if take > 0 {
                out[..take].copy_from_slice(&self.bytes[at as usize..at as usize + take])
            }
            take as i32
        };
        println!("{} {at} {} {n}", if self.book { "B" } else { "R" }, out.len());
        n
    }
}
struct Metrics {
    coverage: bool,
}
impl Font for Metrics {
    fn advance(&mut self, c: u32, s: u16, f: u8) -> i16 {
        let a = (s / 2 + u16::from(f & BOLD != 0)) as i16;
        println!("FA {c} {s} {f} {a}");
        a
    }
    fn line_height(&mut self, s: u16) -> i16 {
        println!("FH {s} {s}");
        s as i16
    }
    fn ascent(&mut self, s: u16) -> i16 {
        println!("FB {s} {s}");
        s as i16
    }
    fn covers(&mut self, c: u32) -> bool {
        println!("FC {c} {}", u8::from(self.coverage));
        self.coverage
    }
    fn kerning(&mut self, a: u32, b: u32, s: u16, f: u8) -> i16 {
        let k = if a == 65 && b == 86 { -1 } else { 0 };
        println!("FK {a} {b} {s} {f} {k}");
        k
    }
    fn ligature(&mut self, a: u32, b: u32, f: u8) -> u32 {
        let l = if a == 102 && b == 105 { 0xfb01 } else { 0 };
        println!("FL {a} {b} {f} {l}");
        l
    }
}
struct Output {
    stop: u32,
}
impl Sink for Output {
    fn anchor(&mut self, h: u32, c: u32) {
        println!("A {h} {c}")
    }
    fn page(&mut self, p: Page<'_>) -> bool {
        println!(
            "P {} {} {} {} {}",
            p.index,
            p.char_start,
            p.runs.len(),
            p.images.len(),
            p.links.len()
        );
        for r in p.runs {
            println!(
                "T {} {} {} {} {}",
                hex(r.text.get(p.bytes)),
                r.x,
                r.baseline,
                r.size,
                r.flags
            )
        }
        for i in p.images {
            println!(
                "I {} {} {} {} {}",
                hex(i.href.get(p.bytes)),
                i.x,
                i.y,
                i.width,
                i.height
            )
        }
        for l in p.links {
            println!(
                "K {} {} {} {} {} {}",
                hex(l.target.get(p.bytes)),
                hex(l.fragment.get(p.bytes)),
                l.x,
                l.y,
                l.width,
                l.height
            )
        }
        p.index + 1 != self.stop
    }
}
struct Catalog {
    entry: Checked,
}
impl newt_core::chapter_layout::images::Catalog for Catalog {
    fn member(&self, _: &mut dyn BookSource, path: &[u8]) -> Result<Checked, Status> {
        (path == b"img.png").then_some(self.entry).ok_or(Status::NotFound)
    }
    fn member_by_hash(&self, _: &mut dyn BookSource, h: u32) -> Result<Checked, Status> {
        (h == newt_core::css::hash(b"img.png")).then_some(self.entry).ok_or(Status::NotFound)
    }
}
struct Policy { enabled: bool, cancel: i64, polls: i64 }
impl newt_core::xml_session::ProcessingPolicy for Policy {
    fn cancelled(&mut self) -> bool {
        let at = self.polls; self.polls += 1;
        let yes = at == self.cancel;
        println!("Q {at} {}", u8::from(yes)); yes
    }
}
enum Selection<'a> {
    Ordinary(newt_core::xml_session::NoPolicy),
    Producer(newt_core::xml_session::BorrowedPolicy<'a>),
}
impl<'a> Selection<'a> {
    fn new(p: &'a mut Policy) -> Self {
        if p.enabled { Self::Producer(newt_core::xml_session::BorrowedPolicy(p)) }
        else { Self::Ordinary(newt_core::xml_session::NoPolicy) }
    }
}
impl newt_core::xml_session::SessionPolicy<expat::Expat> for Selection<'_> {
    fn cancelled(&mut self) -> bool {
        use newt_core::xml_session::{SessionPolicy, BorrowedPolicy, NoPolicy};
        match self {
            Self::Ordinary(p) => <NoPolicy as SessionPolicy<expat::Expat>>::cancelled(p),
            Self::Producer(p) => <BorrowedPolicy<'_> as SessionPolicy<expat::Expat>>::cancelled(p),
        }
    }
    fn create(&mut self, b: &mut expat::Expat) -> bool {
        match self { Self::Ordinary(p) => p.create(b), Self::Producer(p) => p.create(b) }
    }
    fn permits_feed(&self, b: &mut expat::Expat, count: u64, next: u64) -> bool {
        match self { Self::Ordinary(p) => p.permits_feed(b,count,next), Self::Producer(p) => p.permits_feed(b,count,next) }
    }
}
fn run(line: &str) {
    let p: Vec<_> = line.split(' ').collect();
    assert!((13..=20).contains(&p.len()));
    assert!(matches!(p[0], "L" | "X" | "T"));
    let producer = p[0] == "X";
    let mut policy = Policy { enabled: producer, cancel: p.get(17).map_or(-1, |s| if *s == "-1" { -1 } else { i64::from(num(s)) }), polls: 0 };
    let data = decode(p[1]);
    let mut source = Source {
        bytes: data,
        max: num(p[10]) as usize,
        fail: if p[11] == "-1" {
            -1
        } else {
            i64::from(num(p[11]))
        },
        calls: 0,
        book: false,
    };
    assert!(source.max > 0);
    let mut params = Params::default();
    params.width = num(p[2]).try_into().unwrap();
    params.height = num(p[3]).try_into().unwrap();
    params.base_size = num(p[4]).try_into().unwrap();
    params.left = 0;
    params.right = 0;
    params.top = 0;
    params.bottom = 0;
    params.align = match p[5] {
        "left" => TextAlign::Left,
        "right" => TextAlign::Right,
        "center" => TextAlign::Center,
        "justify" => TextAlign::Justify,
        _ => panic!("align"),
    };
    params.focus = match p[6] {
        "0" => false,
        "1" => true,
        _ => panic!("focus"),
    };
    let language = p
        .get(16)
        .filter(|s| **s != "-")
        .map(|s| std::ffi::CString::new(*s).expect("language NUL"));
    if let Some(language) = language.as_deref() {
        params.language = Some(language);
    }
    let hypdata = p.get(15).map_or_else(Vec::new, |s| decode(s));
    let mut hyphenator = newt_core::hyphenator::Hyphenator::new();
    if !hypdata.is_empty() {
        assert!(hyphenator.init(Some(&hypdata)))
    }
    let min = num(p[7]);
    let max = num(p[8]);
    let mut output = Output { stop: num(p[9]) };
    let calls = num(p[12]);
    assert!(calls <= 1024);
    // Host allocations are only the caller-owned resource fixture. No heap in engine.
    let mut text = vec![0; p.get(18).map_or(8192, |s| num(s) as usize)];
    let mut breaks = vec![0; 8192];
    let mut levels = vec![0; 8192];
    let mut spans = vec![Span::default(); 128];
    let mut lines = vec![Line::default(); 512];
    let mut elements = vec![Element::default(); 24];
    let mut runs = vec![Run::default(); 768];
    let mut images = vec![Image::default(); 16];
    let mut links = vec![Link::default(); 24];
    let mut page = vec![0; p.get(19).map_or(24576, |s| num(s) as usize)];
    let mut style = vec![0; 12288];
    let mut pl = vec![ParLink::default(); 8];
    let mut rules = vec![CssRule::default(); 512];
    let mut builder = CssStylesheetBuilder::default();
    assert!(builder.begin(&mut rules));
    let storage = Storage {
        text: &mut text,
        breaks: &mut breaks,
        levels: &mut levels,
        spans: &mut spans,
        lines: &mut lines,
        elements: &mut elements,
        runs: &mut runs,
        images: &mut images,
        links: &mut links,
        page: &mut page,
        style_text: &mut style,
        paragraph_links: &mut pl,
    };
    let mut backend = expat::Expat::default();
    let entry = Checked::raw(source.bytes.len() as u32);
    let mut buffer = [0; 2048];
    let mut filter = EntityFilter::default();
    let mut book = Source {
        bytes: if p.len() >= 14 { decode(p[13]) } else { vec![] },
        max: 4096,
        fail: -1,
        calls: 0,
        book: true,
    };
    let catalog = Catalog {
        entry: Checked::raw(book.bytes.len() as u32),
    };
    let catalog_ref = if p.len() >= 14 && p[13] != "-" {
        Some(&catalog as &dyn newt_core::chapter_layout::images::Catalog)
    } else {
        None
    };
    let mut skip = [0; 1024];
    let mut path_scratch = vec![0; 65536];
    let mut table = vec![newt_core::chapter_layout::images::Probed::default(); 256];
    let count = if p[0] == "T" { 0 } else {
        let mut access = newt_core::chapter_layout::images::BookImages {
            source: &mut book,
            catalog: catalog_ref,
            directory: b"",
            decoder: None,
            skip: &mut skip,
            path_scratch: &mut path_scratch,
            table: &[],
        };
        newt_core::chapter_layout::images::prescan(
            &mut source,
            &entry,
            &mut backend,
            None,
            &mut buffer,
            &mut filter,
            &mut access,
            &mut table,
            Selection::new(&mut policy),
        )
    };
    let mut imgs = newt_core::chapter_layout::images::BookImages {
        source: &mut book,
        catalog: catalog_ref,
        directory: b"",
        decoder: None,
        skip: &mut skip,
        path_scratch: &mut path_scratch,
        table: &table[..count],
    };
    let (mut font, mut breaker) = (
        Metrics {
            coverage: p.get(14).is_none_or(|s| *s == "1"),
        },
        linebreak::NativeBreak,
    );
    let mut execution = newt_core::chapter_layout::types::SplitBackend {
        font: &mut font, sink: &mut output,
    };
    let mut engine = Engine::new(
        params,
        storage,
        &mut execution,
        &mut breaker,
        &mut imgs,
        if hypdata.is_empty() {
            None
        } else {
            Some(&hyphenator)
        },
        builder,
    )
    .unwrap();
    if p[0] == "T" {
        println!("O Ok 0 Idle");
        let status = newt_core::chapter_layout::plain::layout(&mut source, &mut engine, &mut [0;4096]);
        println!("S {} 0 Done {} {}", status.name(), engine.pages, engine.chars);
        println!("END"); return;
    }
    let stream = XmlStream::with_policy(&mut source, None, &mut backend, Selection::new(&mut policy));
    let mut session = ChapterSession::new(stream);
    let status = session.begin(
        &entry,
        Resources {
            buffer: Some(&mut buffer),
            filter: Some(&mut filter),
        },
        &engine,
    );
    println!(
        "O {} {} {:?}",
        status.name(),
        session.bytes_consumed(),
        session.state()
    );
    let mut handler = LayoutHandler {
        engine: &mut engine,
    };
    for _ in 0..calls {
        let st = session.step_bounded(&mut handler, min, max);
        println!(
            "S {} {} {:?} {} {}",
            st.name(),
            session.bytes_consumed(),
            session.state(),
            handler.engine.pages,
            handler.engine.chars
        )
    }
    session.abort();
    println!("END");
}
fn main() {
    // Malformed protocol assertions use exit2; a successful reply has no diagnostics.
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input).unwrap();
        assert!(input.ends_with('\n') && !input.contains('\r'));
        input.pop();
        assert!(!input.contains('\n'));
        run(&input);
    });
    if result.is_err() {
        std::process::exit(2)
    }
}
