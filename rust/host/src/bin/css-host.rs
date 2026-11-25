//! Strict host-only transport. Heap construction here is not target runtime.
use newt_core::member_check::Checked;
use newt_core::{
    inflate::{BitBuffer, State},
    zip_container::{Entry, InflateScratch},
    zip_window::source::BookSource,
};
use newt_core::css::*;
use std::io::{self, BufRead};
fn number<T: std::str::FromStr>(s: &str) -> Result<T, ()> {
    if s.is_empty() || s.bytes().any(|b| !b.is_ascii_digit()) {
        return Err(());
    }
    s.parse().map_err(|_| ())
}
fn signed(s: &str) -> Result<i64, ()> {
    if let Some(t) = s.strip_prefix('-') {
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
    if s.len() % 2 != 0 || s.len() > 2_097_152 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}
fn decl(d: &CssDecl) {
    println!(
        "DECL {} {} {} {} {} {} {} {} {} {} {} {}",
        d.size_pct,
        u8::from(d.size_root_relative),
        d.weight_bold,
        d.style_italic,
        d.align as u8,
        d.text_indent_pct,
        d.margin_left_pct,
        d.margin_top_pct,
        d.margin_bottom_pct,
        d.display_none,
        d.underline,
        d.vert_align
    );
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
    let mut b = CssStylesheetBuilder::default();
    let mut inline = None;
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|_| ())?;
        if line.len() > 2_100_000 {
            return Err(());
        }
        let p: Vec<_> = line.split_ascii_whitespace().collect();
        match p.as_slice() {
            ["B", slots] => {
                let n = number::<usize>(slots)?;
                if n > 1024 {
                    return Err(());
                }
                let storage = Box::leak(vec![CssRule::default(); n].into_boxed_slice());
                println!("BEGIN {}", u8::from(b.begin(storage)));
            }
            ["T", text] => {
                let v = bytes(text)?.ok_or(())?;
                b.add_text(&v);
            }
            ["I", text] => {
                let v = bytes(text)?;
                let d = parse_inline_style(v.as_deref());
                decl(&d);
                inline = Some(d);
            }
            ["A", text] => {
                let v = bytes(text)?;
                let d = parse_inline_style(v.as_deref());
                let base = inline.get_or_insert_with(CssDecl::default);
                base.apply_over(&d);
                decl(base);
            }
            ["C", elem, classes, use_inline] => {
                let e = bytes(elem)?;
                let c = bytes(classes)?;
                let u = number::<u8>(use_inline)?;
                if u > 1 {
                    return Err(());
                }
                decl(&cascade_for(
                    &b.finish(),
                    e.as_deref(),
                    c.as_deref(),
                    if u == 1 { inline.as_ref() } else { None },
                ));
            }
            ["F"] => {
                let s = b.finish();
                println!(
                    "SHEET {} {} {}",
                    s.rules.len(),
                    s.content_hash,
                    b.skipped_sheets()
                );
                for r in s.rules {
                    println!("RULE {} {}", r.elem_hash, r.class_hash);
                    decl(&r.decl);
                }
            }
            [
                "S",
                data,
                method,
                comp,
                uncomp,
                offset,
                max,
                fail,
                ret,
                decode,
                text_cap,
            ] => {
                let data = bytes(data)?.ok_or(())?;
                let method = number::<u16>(method)?;
                let compressed_size = number(comp)?;
                let uncompressed_size = number(uncomp)?;
                let local_header_offset = number(offset)?;
                let max = number(max)?;
                let fail = signed(fail)?;
                let ret = i32::try_from(signed(ret)?).map_err(|_| ())?;
                let decode = number::<u8>(decode)?;
                let cap = number::<usize>(text_cap)?;
                if decode > 1 || cap > 65537 || ret > 0 || fail < -1 {
                    return Err(());
                }
                let mut source = Source {
                    bytes: data,
                    max,
                    fail,
                    ret,
                    calls: 0,
                    trace: Vec::new(),
                };
                let e = Checked::unverified(Entry {
                    method,
                    compressed_size,
                    uncompressed_size,
                    local_header_offset,
                    ..Entry::default()
                });
                let mut state = Box::new(State::new(BitBuffer::Bits32));
                let mut dict = Box::new([0; 32768]);
                let mut input = [0; 2048];
                let mut text = vec![0; cap];
                let scratch = if decode == 1 {
                    Some(InflateScratch {
                        state: &mut state,
                        dictionary: &mut dict,
                        input: &mut input,
                    })
                } else {
                    None
                };
                let status = b.add_sheet(&mut source, &e, scratch, &mut text);
                println!("STATUS {}", status as u8);
                println!("SCRATCH 1");
                for t in source.trace {
                    println!("{t}");
                }
            }
            _ => return Err(()),
        }
    }
    Ok(())
}
fn main() {
    if run().is_err() {
        std::process::exit(2);
    }
}
