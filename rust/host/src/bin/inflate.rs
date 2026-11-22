use newt_core::inflate::{BitBuffer, State};
use std::io::{self, BufRead, Write};
fn nat(s: &str) -> usize {
    assert!(!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    assert!(s == "0" || !s.starts_with('0'));
    s.parse().expect("integer overflow")
}
fn hex(s: &str) -> Vec<u8> {
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
        .chunks_exact(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect()
}
fn main() {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    fn line(reader: &mut impl BufRead) -> Option<String> {
        let mut bytes = Vec::new();
        let count = reader.read_until(b'\n', &mut bytes).unwrap();
        if count == 0 {
            return None;
        }
        assert!(bytes.last() == Some(&b'\n') && !bytes.contains(&b'\r') && !bytes.contains(&0));
        bytes.pop();
        Some(String::from_utf8(bytes).unwrap())
    }
    let header = line(&mut reader).expect("header");
    let head: Vec<_> = header.split(' ').collect();
    assert_eq!(head.len(), 3);
    assert_eq!(head[0], "TINFL1");
    let mode = match head[1] {
        "32" => BitBuffer::Bits32,
        "64" => BitBuffer::Bits64,
        _ => panic!("mode"),
    };
    let mut output = vec![0u8; nat(head[2])];
    // Box belongs only to this std host driver, never the no_std engine/probe.
    let mut state = Box::new(State::new(mode));
    while let Some(line) = line(&mut reader) {
        if line == "RESET" {
            state.reset();
            continue;
        }
        let f: Vec<_> = line.split(' ').collect();
        assert_eq!(f.len(), 5);
        assert_eq!(f[0], "CALL");
        let flags = u32::try_from(nat(f[1])).unwrap();
        let offset = nat(f[2]);
        let cap = nat(f[3]);
        let input = hex(f[4]);
        let r = state.decompress(&input, &mut output, offset, cap, flags);
        let bytes = if r.produced == 0 {
            "-".into()
        } else {
            output[offset..offset + r.produced]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        writeln!(
            &mut stdout,
            "RESULT {} {} {} {} {}",
            r.status as i32, r.consumed, r.produced, r.adler32, bytes
        )
        .unwrap();
        stdout.flush().unwrap();
    }
}
