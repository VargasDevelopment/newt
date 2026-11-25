//! Strict host transport; the core remains no_std and allocation-free.
use newt_core::member_check::Checked;
use newt_core::{
    inflate::{BitBuffer, State},
    zip_container::*,
    zip_window::{arena::Arena, source::BookSource},
};
use std::{
    cell::RefCell,
    io::{self, Read},
};
fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Vec<u8> {
    if s == "-" {
        return vec![];
    }
    assert!(
        s.len() % 2 == 0 && s.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid hex"
    );
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
struct Source {
    bytes: Vec<u8>,
    max: usize,
    fail: i64,
    fail_return: i32,
    calls: i64,
    trace: RefCell<Vec<String>>,
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.trace
            .borrow_mut()
            .push(format!("SIZE {}", self.bytes.len()));
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let n = if self.calls == self.fail {
            self.fail_return
        } else {
            let bytes = usize::try_from(at)
                .ok()
                .and_then(|at| self.bytes.get(at..))
                .unwrap_or(&[]);
            let n = bytes.len().min(out.len()).min(self.max);
            out[..n].copy_from_slice(&bytes[..n]);
            n as i32
        };
        self.calls += 1;
        self.trace
            .borrow_mut()
            .push(format!("READ {at} {} {n}", out.len()));
        n
    }
}
fn number<T: std::str::FromStr>(s: Option<&str>) -> T
where
    T::Err: std::fmt::Debug,
{
    s.expect("missing field").parse().expect("invalid number")
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.is_empty() || args == ["--reopen"]);
    let reopen = !args.is_empty();
    let mut text = String::new();
    io::stdin().read_to_string(&mut text).unwrap();
    for line in text.lines() {
        let mut f = line.split_whitespace();
        let bytes = unhex(f.next().expect("archive"));
        let max = number(f.next());
        let fail = number(f.next());
        let fail_return = number(f.next());
        assert!(
            fail_return <= 0,
            "fault injection may only return zero or negative"
        );
        let name_cap: usize = number(f.next());
        let entry_cap: usize = number(f.next());
        let scratch: u8 = number(f.next());
        assert!(scratch <= 1);
        let sizes: Vec<usize> = f
            .next()
            .expect("read sizes")
            .split(',')
            .map(|v| v.parse().unwrap())
            .collect();
        assert!(sizes.iter().all(|n| *n <= 1_048_576));
        let raw: i64 = number(f.next());
        assert!(raw >= -1 && raw <= u32::MAX as i64);
        assert!(f.next().is_none(), "extra fields");
        let mut source = Source {
            bytes,
            max,
            fail,
            fail_return,
            calls: 0,
            trace: RefCell::new(vec![]),
        };
        let mut names = vec![0; name_cap];
        let mut arena = Arena::new(&mut names);
        let mut slots = vec![Entry::default(); entry_cap];
        let mut scan = vec![0; 4099];
        let mut seq = vec![0; 1024];
        let mut cat = Catalog::new(&mut source, &mut arena, &mut slots);
        let mut entries = vec![];
        if raw < 0 {
            let status = cat.open(
                scan.as_mut_slice().try_into().unwrap(),
                seq.as_mut_slice().try_into().unwrap(),
            );
            println!("CAT {} {}", status as u8, cat.entry_count());
            for i in 0..cat.entry_count() {
                let e = *cat.entry(i).unwrap();
                let name = cat.name(&e).unwrap();
                println!(
                    "ENTRY {i} {}00 {} {} {} {} {}",
                    hex(name),
                    e.name_hash,
                    e.compressed_size,
                    e.uncompressed_size,
                    e.local_header_offset,
                    e.method
                );
                println!(
                    "FIND {i} {} {}",
                    cat.find(Some(name)).map(|e| e.local_header_offset).unwrap(),
                    cat.find_by_hash(e.name_hash)
                        .map(|e| e.local_header_offset)
                        .unwrap()
                );
                entries.push(e);
            }
            if reopen {
                let status = cat.open(
                    scan.as_mut_slice().try_into().unwrap(),
                    seq.as_mut_slice().try_into().unwrap(),
                );
                println!("REOPEN {} {}", status as u8, cat.entry_count());
                entries.clear();
            }
            println!(
                "RUST_CAP {} {} {} {}",
                entry_cap,
                cat.arena_used(),
                name_cap,
                core::mem::size_of::<Entry>()
            );
        } else {
            entries.push(Entry::raw(raw as u32));
            println!("RAW {raw}");
        }
        drop(cat);
        // Host-only heap allocation; target passes initialized borrowed storage.
        let mut state = Box::new(State::new(BitBuffer::Bits64));
        let mut dict = vec![0; 32768];
        let mut input = vec![0; 2048];
        for (i, e) in entries.iter().enumerate() {
            let storage = if scratch == 1 {
                Some(InflateScratch {
                    state: &mut state,
                    dictionary: dict.as_mut_slice().try_into().unwrap(),
                    input: input.as_mut_slice().try_into().unwrap(),
                })
            } else {
                None
            };
            // The oracle reads raw catalog entries, so the harness skips the check.
            let e = Checked::unverified(*e);
            let mut r = Reader::new(&mut source, storage);
            let status = r.open(&e);
            println!("OPEN {i} {}", status as u8);
            if status != Status::Ok {
                continue;
            }
            for &n in &sizes {
                let mut out = vec![0xa5; n];
                let got = r.read(&mut out);
                println!(
                    "OUT {i} {n} {got} {} {}",
                    r.total_produced(),
                    if out.is_empty() {
                        "-".into()
                    } else {
                        hex(&out)
                    }
                );
            }
        }
        for t in source.trace.into_inner() {
            println!("{t}");
        }
        println!("END");
    }
}
