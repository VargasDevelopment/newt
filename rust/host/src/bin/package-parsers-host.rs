#[path = "../xml_session/expat.rs"]
mod expat;
#[cfg(test)]
#[path = "../package_parsers/native_tests.rs"]
mod native_tests;
use newt_core::package_parsers::*;
use newt_core::xml_session::PARSE_CHUNK;
use newt_core::member_check::Checked;
use newt_core::zip_container::{Entry, InflateScratch, Status};
use newt_core::zip_window::{
    arena::{Arena, CStringRef},
    source::BookSource,
};
use std::io::{self, Read};
fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".into();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode(s: &str) -> Result<Vec<u8>, ()> {
    if s == "-" {
        return Ok(vec![]);
    }
    if s.is_empty()
        || s.len() % 2 != 0
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(());
    }
    s.as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).map_err(|_| ())?, 16).map_err(|_| ()))
        .collect()
}
fn number(s: &str) -> Result<usize, ()> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(());
    }
    let n = s.parse().map_err(|_| ())?;
    if n > 1048576 { Err(()) } else { Ok(n) }
}
struct Source {
    bytes: Vec<u8>,
    max: usize,
    fail: i64,
    calls: i64,
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
            let tail = self.bytes.get(at as usize..).unwrap_or_default();
            let n = tail.len().min(self.max).min(out.len());
            out[..n].copy_from_slice(&tail[..n]);
            n as i32
        };
        println!("R {at} {} {n}", out.len());
        n
    }
}
fn string(arena: &Arena<'_>, value: Option<CStringRef>, null: &str) -> String {
    value
        .and_then(|v| arena.c_string(v))
        .map(hex)
        .unwrap_or_else(|| null.into())
}
fn run(line: &str) -> Result<(), ()> {
    let p: Vec<_> = line.split(' ').collect();
    if ((p.len() != 8 || p[0] != "P") && (p.len() != 11 || p[0] != "Z"))
        || !["C", "P", "N", "X"].contains(&p[1])
    {
        return Err(());
    }
    let data = decode(p[2])?;
    let dir = decode(p[3])?;
    if data.len() > 1048576 || dir.len() >= 512 || dir.contains(&0) {
        return Err(());
    }
    let max = number(p[4])?;
    if max == 0 {
        return Err(());
    }
    let fail = if p[5] == "-1" {
        -1
    } else {
        number(p[5])? as i64
    };
    let cap = number(p[6])?;
    let available = match p[7] {
        "0" => false,
        "1" => true,
        _ => return Err(()),
    };
    let extended = p[0] == "Z";
    let mut entry = Entry::raw(data.len() as u32);
    let decoder_available = if extended {
        if data.len() < 30 {
            return Err(());
        }
        let method = number(p[8])?;
        if method > u16::MAX as usize {
            return Err(());
        }
        entry.local_header_offset = 0;
        entry.method = method as u16;
        entry.compressed_size = (data.len() - 30) as u32;
        entry.uncompressed_size = number(p[9])? as u32;
        match p[10] {
            "0" => false,
            "1" => true,
            _ => return Err(()),
        }
    } else {
        false
    };
    let entry = Checked::unverified(entry);
    let mut inflater = (entry.method == 8 && decoder_available).then(|| {
        (
            Box::new(newt_core::inflate::State::new(newt_core::inflate::BitBuffer::Bits64)),
            Box::new([0u8; newt_core::inflate::DICTIONARY_SIZE]),
            Box::new([0u8; newt_core::zip_container::INPUT_SIZE]),
        )
    });
    let mut source = Source {
        bytes: data,
        max,
        fail,
        calls: 0,
    };
    let mut backend = expat::Expat::default();
    let mut buffer = [0; PARSE_CHUNK];
    let mut workflow = Workflow {
                policy: None,
        source: &mut source,
        backend: &mut backend,
        decoder: inflater
            .as_mut()
            .map(|(state, dictionary, input)| InflateScratch {
                state: state.as_mut(),
                dictionary: dictionary.as_mut(),
                input: input.as_mut(),
            }),
        buffer: if available { Some(&mut buffer) } else { None },
        filter: None,
    };
    let mut backing = vec![0; cap];
    let mut arena = Arena::new(&mut backing);
    let mut manifest = vec![ManifestItem::default(); 4096];
    let mut hashes = vec![0; 4096];
    let mut spine = vec![0; 4096];
    let mut toc = vec![TocEntry::default(); 4096];
    let mut count = 0;
    let mut package = PackageResult::default();
    let mut path = None;
    let status = match p[1] {
        "C" => parse_container(&mut workflow, &entry, &mut arena, &mut path),
        "P" => parse_package(
            &mut workflow,
            &entry,
            &dir,
            &mut arena,
            PackageStorage {
                manifest: &mut manifest,
                spine_hashes: &mut hashes,
                spine: &mut spine,
            },
            &mut package,
        ),
        kind => parse_toc(
            &mut workflow,
            &entry,
            &dir,
            &mut arena,
            if kind == "N" {
                TocKind::Nav
            } else {
                TocKind::Ncx
            },
            &mut toc,
            &mut count,
        ),
    };
    println!("P {}", status.name());
    if status == Status::Ok {
        match p[1] {
            "C" => println!("C {}", string(&arena, path, "~")),
            "P" => {
                let m = package.metadata;
                println!(
                    "M {} {} {} {}",
                    string(&arena, m.title, "-"),
                    string(&arena, m.author, "-"),
                    string(&arena, m.language, "-"),
                    string(&arena, m.identifier, "-")
                );
                for m in &manifest[..package.manifest_count] {
                    println!(
                        "I {} {} {} {} {} {}",
                        string(&arena, m.id, "~"),
                        string(&arena, m.href, "~"),
                        string(&arena, m.media_type, "-"),
                        m.id_hash,
                        u8::from(m.is_nav),
                        u8::from(m.is_cover_image)
                    );
                }
                for index in &spine[..package.spine_count] {
                    println!("S {index}");
                }
                println!(
                    "D {} {}",
                    package.nav_index.map_or(-1, |i| i as i64),
                    package.ncx_index.map_or(-1, |i| i as i64)
                );
            }
            _ => {
                for t in &toc[..count] {
                    println!(
                        "T {} {} {} {}",
                        string(&arena, t.title, "-"),
                        string(&arena, t.href, "-"),
                        string(&arena, t.fragment, "~"),
                        t.depth
                    );
                }
            }
        }
    }
    println!("END");
    Ok(())
}
fn main() {
    let mut input = String::new();
    let result = io::stdin()
        .read_to_string(&mut input)
        .map_err(|_| ())
        .and_then(|_| {
            if input.is_empty() || !input.ends_with('\n') || input.contains('\r') {
                return Err(());
            }
            for line in input[..input.len() - 1].split('\n') {
                run(line)?;
            }
            Ok(())
        });
    if result.is_err() {
        std::process::exit(2);
    }
}
