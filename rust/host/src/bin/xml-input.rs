// Host-only transport. Core library uses no std or heap.
use newt_core::{
    xml_input::*,
    zip_container::{Entry, Reader, Status},
    zip_window::{arena::Arena, source::BookSource},
};
use std::{
    cell::Cell,
    io::{self, BufRead},
    rc::Rc,
};
fn decode(s: &str) -> Vec<u8> {
    if s == "-" {
        return vec![];
    }
    assert!(!s.is_empty() && s.len() % 2 == 0 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn hex(b: &[u8]) -> String {
    if b.is_empty() {
        "-".into()
    } else {
        b.iter().map(|b| format!("{b:02x}")).collect()
    }
}
fn num(s: &str) -> usize {
    assert!(!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    let n = s.parse().unwrap();
    assert!(n <= 1048576);
    n
}
fn nonpositive(s: &str) -> i32 {
    assert!(
        s == "0"
            || (s.starts_with('-') && s[1..].bytes().all(|b| b.is_ascii_digit()) && s.len() > 1)
    );
    s.parse().unwrap()
}
struct Source {
    bytes: Vec<u8>,
    max: usize,
    fail: i32,
    ret: i32,
    calls: Rc<Cell<usize>>,
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let call = self.calls.get();
        self.calls.set(call + 1);
        if call as i32 == self.fail {
            return self.ret;
        }
        let bytes = self.bytes.get(at as usize..).unwrap_or(&[]);
        let n = bytes.len().min(out.len()).min(self.max);
        out[..n].copy_from_slice(&bytes[..n]);
        n as i32
    }
}
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    if std::panic::catch_unwind(|| {
        for line in io::stdin().lock().lines() {
            run(&line.unwrap());
        }
    })
    .is_err()
    {
        std::process::exit(2);
    }
}
fn run(line: &str) {
    let t: Vec<_> = line.split_whitespace().collect();
    assert!(!t.is_empty());
    match t[0] {
        "F" => {
            assert_eq!(t.len(), 7);
            let bytes = decode(t[1]);
            let max = num(t[2]);
            let fail = if t[3] == "-1" { -1 } else { num(t[3]) as i32 };
            let ret = nonpositive(t[4]);
            // Field 5 is reserved: require 0, allowing protocol growth without ambiguity.
            assert_eq!(t[5], "0");
            let actions: Vec<_> = t[6]
                .split(',')
                .map(|a| if a == "r" { None } else { Some(num(a)) })
                .collect();
            let calls = Rc::new(Cell::new(0));
            let entry = Entry::raw(bytes.len() as u32);
            let mut source = Source {
                bytes,
                max,
                fail,
                ret,
                calls: calls.clone(),
            };
            let mut reader = Reader::new(&mut source, None);
            assert_eq!(reader.open(&entry), Status::Ok);
            let mut f = EntityFilter::new();
            for a in actions {
                if let Some(cap) = a {
                    let mut out = vec![0xa5; cap];
                    let n = f.read(&mut reader, &mut out);
                    println!(
                        "F {n} {} {} {}",
                        hex(&out),
                        calls.get(),
                        reader.total_produced()
                    );
                } else {
                    f.reset();
                    println!("RESET");
                }
            }
        }
        "D" => {
            assert_eq!(t.len(), 3);
            let p = decode(t[1]);
            let mut out = vec![0xa5; num(t[2])];
            let ok = dir_name(&p, &mut out);
            println!("D {} {}", u8::from(ok), hex(&out));
        }
        "P" => {
            assert_eq!(t.len(), 5);
            let cap = num(t[1]);
            let base = (t[2] != "null").then(|| decode(t[2]));
            let href = (t[3] != "null").then(|| decode(t[3]));
            assert!(matches!(t[4], "0" | "1"));
            let mut mem = vec![0xa5; cap];
            let mut arena = Arena::new(&mut mem);
            let mut fragment = None;
            let path = resolve_href(
                &mut arena,
                base.as_deref(),
                href.as_deref(),
                if t[4] == "1" {
                    Some(&mut fragment)
                } else {
                    None
                },
            );
            let show = |r: Option<newt_core::zip_window::arena::ByteRange>| {
                r.map_or("null".into(), |r| hex(arena.view(r).unwrap()))
            };
            println!(
                "P {} {} {} {} {}",
                show(path),
                show(fragment),
                arena.used(),
                arena.high_water(),
                arena.failed_alloc_size()
            );
        }
        "L" => {
            assert_eq!(t.len(), 3);
            let q = decode(t[1]);
            let l = decode(t[2]);
            println!("L {} {}", hex(local_name(&q)), u8::from(local_is(&q, &l)));
        }
        "H" => {
            assert_eq!(t.len(), 3);
            let v = (t[1] != "null").then(|| decode(t[1]));
            println!("H {}", u8::from(has_token(v.as_deref(), &decode(t[2]))));
        }
        "A" | "E" => {
            assert!(t.len() >= 3 && (t.len() - 3) % 2 == 0);
            assert!(matches!(t[1], "local" | "exact"));
            let name = decode(t[2]);
            let fields: Vec<_> = t[3..].iter().map(|s| decode(s)).collect();
            let atts: Vec<_> = fields
                .chunks_exact(2)
                .map(|v| (v[0].as_slice(), v[1].as_slice()))
                .collect();
            if t[0] == "A" {
                let v = if t[1] == "local" {
                    attr_local(&atts, &name)
                } else {
                    attr_exact(&atts, &name)
                };
                println!("A {}", v.map_or("null".into(), hex));
            } else {
                let mut scan = EncryptionScan {
                    content_encrypted: t[1] == "exact",
                };
                scan.on_start_element(&name, &atts);
                println!("E {}", u8::from(scan.content_encrypted));
            }
        }
        "T" => {
            assert!(t.len() > 1);
            let mut capture = TextCapture::default();
            // First begin makes every serialized byte defined, including C++'s otherwise uninitialized buffer.
            capture.begin();
            for action in &t[1..] {
                let mut trimmed = "null".into();
                match *action {
                    "b" => capture.begin(),
                    "e" => capture.end(),
                    "t" => trimmed = hex(capture.trimmed()),
                    "n" => capture.add_event(&[], -1),
                    s => {
                        assert!(s.starts_with('a'));
                        capture.add(&decode(&s[1..]));
                    }
                }
                println!(
                    "T {} {} {} {}",
                    capture.len,
                    u8::from(capture.active),
                    trimmed,
                    hex(&capture.buf[..=capture.len])
                );
            }
        }
        _ => panic!("invalid opcode"),
    }
    println!("END");
}
