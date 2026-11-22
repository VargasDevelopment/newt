//! Host transport only. Native Expat callbacks are the existing audited seam.
#[path = "../xml_session/expat.rs"]
mod expat;
#[cfg(test)]
#[path = "../epub_admission/native_tests.rs"]
mod native_tests;
use newt_core::{
    admission, canonical,
    inflate::{BitBuffer, DICTIONARY_SIZE, State},
    package_parsers::{ManifestItem, PackageResult},
    xml_input::EntityFilter,
    zip_container::{Catalog, Entry, InflateScratch, Status},
    zip_window::{arena::Arena, source::BookSource},
};
use std::io::{self, Read};

fn number(text: &str) -> Result<usize, ()> {
    if text.is_empty()
        || !text.bytes().all(|b| b.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err(());
    }
    text.parse::<usize>()
        .map_err(|_| ())
        .and_then(|n| if n <= 2097152 { Ok(n) } else { Err(()) })
}
fn decode(text: &str) -> Result<Vec<u8>, ()> {
    if text == "-" {
        return Ok(Vec::new());
    }
    if text.is_empty()
        || text.len() % 2 != 0
        || text.len() > 4194304
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(());
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).map_err(|_| ())?, 16).map_err(|_| ()))
        .collect()
}
fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
struct Source {
    bytes: Vec<u8>,
    max: usize,
    fail: Option<usize>,
    calls: usize,
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let call = self.calls;
        self.calls += 1;
        let got = if self.fail == Some(call) {
            -1
        } else {
            let tail = usize::try_from(at)
                .ok()
                .and_then(|n| self.bytes.get(n..))
                .unwrap_or_default();
            let n = tail.len().min(self.max).min(out.len());
            out[..n].copy_from_slice(&tail[..n]);
            n as i32
        };
        println!("R {at} {} {got}", out.len());
        got
    }
}
fn on_yield() {
    println!("Y");
}
fn observe(e: admission::WorkEvent) {
    println!(
        "E {} {} {} {} {} {} {} {}",
        e.kind as u8,
        e.phase as u8,
        e.member,
        e.compressed_bytes,
        e.expanded_bytes,
        e.result,
        e.method,
        e.category
    );
}
fn metadata(book: &canonical::Book<'_, '_, Source, expat::Expat>) {
    let PackageResult {
        metadata: m,
        manifest_count,
        spine_count,
        ..
    } = book.package();
    let bytes = |handle: Option<newt_core::zip_window::arena::CStringRef>| {
        handle.and_then(|h| book.arena().c_string(h)).unwrap_or(b"")
    };
    println!(
        "M {} {} {} {}",
        hex(bytes(m.title)),
        hex(bytes(m.author)),
        hex(bytes(m.language)),
        hex(bytes(m.identifier))
    );
    println!(
        "P {manifest_count} {spine_count} {}",
        u8::from(book.navigation_is_epub3())
    );
}
fn request(line: &str) -> Result<(), ()> {
    let p: Vec<_> = line.split(' ').collect();
    if p.len() != 9 || p[0] != "EA1" || !matches!(p[1], "A" | "O" | "C") {
        return Err(());
    }
    let bytes = decode(p[2])?;
    let cat_cap = number(p[3])?;
    let parse_cap = number(p[4])?;
    let max = number(p[5])?;
    let fail = if p[6] == "-1" {
        None
    } else {
        Some(number(p[6])?)
    };
    let member = number(p[7])?;
    let read_cap = number(p[8])?;
    if max == 0 || read_cap == 0 || read_cap > 32768 {
        return Err(());
    }
    let mut source = Source {
        bytes,
        max,
        fail,
        calls: 0,
    };
    let mut cb = vec![0; cat_cap];
    let mut pb = vec![0; parse_cap];
    let mut catalog = Arena::new(&mut cb);
    let mut parse = Arena::new(&mut pb);
    // These allocations are HOST transport resources, explicitly separate from
    // the translated byte-arena contract. No such stack allocation enters core.
    let mut scan = [0; 4099];
    let mut sequential = [0; 1024];
    let typed = std::env::var("NEWT_TYPED_CASE").unwrap_or_default();
    let mut entries = vec![Entry::default(); if typed == "no-entries" { 0 } else { 1024 }];
    let mut manifest = vec![ManifestItem::default(); if typed == "no-manifest" { 0 } else { 1024 }];
    let mut spine_hashes = vec![0; 1024];
    let mut spine = vec![0; if typed == "no-spine" { 0 } else { 1024 }];
    let mut state = Box::new(State::new(BitBuffer::Bits64));
    let mut dictionary = Box::new([0; DICTIONARY_SIZE]);
    let mut input = [0; 2048];
    let mut chunk = [0; 2048];
    let mut xml_buffer = [0; 2048];
    let mut filter = EntityFilter::new();
    let mut backend = expat::Expat::default();
    backend.fail_create = typed == "fail-backend";
    let status = if p[1] == "C" {
        let mut zip = Catalog::new(&mut source, &mut catalog, &mut entries);
        let status = zip.open(&mut scan, &mut sequential);
        if status != Status::Ok {
            status
        } else if let Some(entry) = zip.entry(member).copied() {
            let decoder = Some(InflateScratch {
                state: &mut state,
                dictionary: &mut dictionary,
                input: &mut input,
            });
            match admission::ChapterSource::open(zip.source(), &entry, decoder, Some(&on_yield), None) {
                Err(error) => error,
                Ok(mut chapter) => {
                    let mut output = vec![0; read_cap];
                    let mut at = 0u64;
                    loop {
                        let n = chapter.read_at(at, &mut output);
                        println!("C {at} {n} {}", hex(&output[..n.max(0) as usize]));
                        if n <= 0 {
                            break;
                        }
                        at += n as u64;
                    }
                    let finished = chapter.finish();
                    println!("F {}", u8::from(finished));
                    Status::Ok
                }
            }
        } else {
            Status::NotFound
        }
    } else {
        let mut resources = canonical::Resources {
                policy: None,
            scan: &mut scan,
            sequential: &mut sequential,
            entries: &mut entries,
            manifest: &mut manifest,
            spine_hashes: &mut spine_hashes,
            spine: &mut spine,
            backend: &mut backend,
            decoder: if typed == "no-decoder" {
                None
            } else {
                Some(InflateScratch {
                    state: &mut state,
                    dictionary: &mut dictionary,
                    input: &mut input,
                })
            },
            chunk: if typed == "no-chunk" {
                None
            } else {
                Some(&mut chunk)
            },
            xml_buffer: if typed == "no-xml" {
                None
            } else {
                Some(&mut xml_buffer)
            },
            filter: if typed == "no-filter" {
                None
            } else {
                Some(&mut filter)
            },
        };
        if p[1] == "A" {
            admission::admit(
                &mut source,
                &mut catalog,
                &mut parse,
                &mut resources,
                Some(&on_yield),
                Some(observe),
            )
        } else {
            match canonical::open(&mut source, &mut catalog, &mut resources, false, None, None) {
                Err(error) => error,
                Ok(book) => {
                    metadata(&book);
                    Status::Ok
                }
            }
        }
    };
    println!("S {}", status as u8);
    println!(
        "D {} {} {} {} {} {}\nEND",
        catalog.used(),
        catalog.high_water(),
        catalog.failed_alloc_size(),
        parse.used(),
        parse.high_water(),
        parse.failed_alloc_size()
    );
    Ok(())
}
fn main() {
    let result = (|| {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text).map_err(|_| ())?;
        if text.is_empty() || !text.ends_with('\n') || text.contains('\r') {
            return Err(());
        }
        for line in text[..text.len() - 1].split('\n') {
            request(line)?;
        }
        Ok(())
    })();
    if result.is_err() {
        std::process::exit(2);
    }
}
