//! Strict host-only transport. Heap construction here is not target runtime.
use newt_core::image_probe::*;
use newt_core::member_check::Checked;
use newt_core::{
    inflate::{BitBuffer, State},
    zip_container::{Entry, InflateScratch},
    zip_window::source::BookSource,
};
use std::io::{self, Read};
fn number<T: std::str::FromStr>(s: &str) -> Result<T, ()> {
    if s.is_empty() || (s.len() > 1 && s.starts_with('0')) || s.bytes().any(|b| !b.is_ascii_digit())
    {
        return Err(());
    }
    s.parse().map_err(|_| ())
}
fn signed(s: &str) -> Result<i64, ()> {
    if let Some(t) = s.strip_prefix('-') {
        if t == "0" {
            return Err(());
        }
        number::<i64>(t)?.checked_neg().ok_or(())
    } else {
        number(s)
    }
}
fn bytes(s: &str) -> Result<Option<Vec<u8>>, ()> {
    if s == "-" {
        return Ok(None);
    }
    if s == "_" {
        return Ok(Some(Vec::new()));
    }
    if s.is_empty()
        || s.len() % 2 != 0
        || s.len() > 2_097_152
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}
struct Source {
    bytes: Vec<u8>,
    max: usize,
    fail: i64,
    ret: i32,
    calls: i64,
    trace: Vec<String>,
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let n = if self.calls == self.fail {
            self.ret
        } else {
            let tail = self
                .bytes
                .get(usize::try_from(at).unwrap_or(usize::MAX)..)
                .unwrap_or(&[]);
            let n = tail.len().min(self.max).min(out.len());
            out[..n].copy_from_slice(&tail[..n]);
            n as i32
        };
        self.calls += 1;
        self.trace.push(format!("READ {at} {} {n}", out.len()));
        n
    }
}
fn run() -> Result<(), ()> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text).map_err(|_| ())?;
    if text.len() > 16_000_000 || !text.ends_with('\n') || text.contains('\r') {
        return Err(());
    }
    for (op, line) in text[..text.len() - 1].split('\n').enumerate() {
        let p: Vec<_> = line.split(' ').collect();
        let [
            "P",
            data,
            method,
            comp,
            uncomp,
            offset,
            max,
            fail,
            ret,
            decode,
            cap,
        ] = p.as_slice()
        else {
            return Err(());
        };
        let data = bytes(data)?.ok_or(())?;
        let e = Checked::unverified(Entry {
            method: number(method)?,
            compressed_size: number(comp)?,
            uncompressed_size: number(uncomp)?,
            local_header_offset: number(offset)?,
            ..Entry::default()
        });
        let fail = signed(fail)?;
        let ret = i32::try_from(signed(ret)?).map_err(|_| ())?;
        let decode = number::<u8>(decode)?;
        let cap = number::<usize>(cap)?;
        if fail < -1 || ret > 0 || decode > 1 || cap > 2048 {
            return Err(());
        }
        let mut source = Source {
            bytes: data,
            max: number(max)?,
            fail,
            ret,
            calls: 0,
            trace: Vec::new(),
        };
        let mut state = Box::new(State::new(BitBuffer::Bits32));
        let mut dictionary = Box::new([0; 32768]);
        let mut input = [0; 2048];
        let mut staging = vec![0; cap];
        let scratch = if decode == 1 {
            Some(InflateScratch {
                state: &mut state,
                dictionary: &mut dictionary,
                input: &mut input,
            })
        } else {
            None
        };
        // Poison proves the public call resets all output even on open/OOM errors.
        let mut out = ImageInfo {
            kind: Kind::Jpeg,
            width: 99,
            height: 98,
            progressive: true,
        };
        let status = probe(&mut source, &e, scratch, &mut staging, &mut out);
        println!(
            "OP {op}\nSTATUS {}\nINFO {} {} {} {}\nSCRATCH 1",
            status as u8,
            out.kind as u8,
            out.width,
            out.height,
            u8::from(out.progressive)
        );
        for row in &source.trace {
            println!("{row}");
        }
        println!("END {}", source.calls);
    }
    Ok(())
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--resources"] {
        println!(
            "RUST_STATE {} {}",
            std::mem::size_of::<State>(),
            std::mem::align_of::<State>()
        );
        println!("RUST_DICTIONARY 32768\nRUST_INPUT 2048\nRUST_STAGING_MIN 1024");
        return;
    }
    if !args.is_empty() || run().is_err() {
        std::process::exit(2);
    }
}
