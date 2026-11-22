use newt_core::{
    image_renderer::*,
    inflate::{BitBuffer, State},
    page_renderer::{ImageRows, SafetyError},
    pngle::*,
};
use std::{
    fmt::Write,
    io::{self, Read},
};
struct NativeMath;
impl Math for NativeMath {
    fn pow(&self, b: f64, e: f64) -> f64 {
        b.powf(e)
    }
}
fn hex(b: &[u8]) -> String {
    let mut s = String::new();
    for n in b {
        write!(s, "{n:02x}").unwrap();
    }
    s
}
struct Trace {
    events: Vec<String>,
    stop: usize,
    rows: usize,
}
impl Callbacks for Trace {
    fn init(&mut self, h: Header) -> Result<(), CallbackError> {
        self.events
            .push(format!("I:{}:{}:{}", h.width, h.height, h.interlace));
        Ok(())
    }
    fn draw(&mut self, x: u32, y: u32, w: u32, h: u32, p: [u8; 4]) -> Result<(), CallbackError> {
        self.events.push(format!("P:{x}:{y}:{w}:{h}:{}", hex(&p)));
        Ok(())
    }
    fn done(&mut self) -> Result<(), CallbackError> {
        self.events.push("D".into());
        Ok(())
    }
}
impl ImageRows for Trace {
    fn on_row(&mut self, y: u16, b: &[u8], w: u16) -> Result<bool, SafetyError> {
        self.events.push(format!("O:{y}:{w}:{}", hex(b)));
        self.rows += 1;
        Ok(self.stop == 0 || self.rows < self.stop)
    }
}
struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
    step: usize,
    fail: usize,
    events: Vec<String>,
}
impl EntryInput for Input<'_> {
    fn read(&mut self, dst: &mut [u8]) -> i32 {
        // ZipEntryReader checks EOF before calling source.readAt.
        if self.at == self.bytes.len() {
            return 0;
        }
        let want = dst.len().min(self.bytes.len() - self.at);
        let n = if self.at >= self.fail {
            -1
        } else {
            let n = want.min(self.step);
            dst[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
            n as i32
        };
        self.events.push(format!("R:{}:{want}:{n}", self.at));
        if n > 0 {
            self.at += n as usize;
        }
        n
    }
}
fn main() {
    if run().is_err() {
        std::process::exit(2)
    }
}
fn run() -> Result<(), String> {
    let mut raw = String::new();
    io::stdin()
        .read_to_string(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() > 2_100_000
        || !raw.ends_with('\n')
        || raw[..raw.len() - 1]
            .bytes()
            .any(|b| b != b' ' && !(33..=126).contains(&b))
    {
        return Err("framing".into());
    }
    let mut t = raw[..raw.len() - 1].split(' ');
    if t.next() != Some("PN1") {
        return Err("version".into());
    }
    fn n(t: &mut std::str::Split<'_, char>, max: usize) -> Result<usize, String> {
        let s = t.next().ok_or("missing")?;
        let v = s.parse::<usize>().map_err(|_| "integer")?;
        if v > max || s != v.to_string() {
            return Err("integer domain".into());
        }
        Ok(v)
    }
    let mode = n(&mut t, 1)?;
    let bits = n(&mut t, 1)?;
    let gamma = n(&mut t, 10000)?;
    let sw = n(&mut t, 65535)?;
    let sh = n(&mut t, 65535)?;
    let dw = n(&mut t, 65535)?;
    let dh = n(&mut t, 65535)?;
    let stop = n(&mut t, 65535)?;
    let step = n(&mut t, 1048576)?;
    let fail = n(&mut t, 1048576)?;
    if mode == 1 && gamma != 0 {
        return Err("wrapper gamma".into());
    }
    if step == 0 {
        return Err("step".into());
    }
    let s = t.next().ok_or("hex")?;
    if s.is_empty()
        || t.next().is_some()
        || s.len() % 2 != 0
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("hex".into());
    }
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect();
    if bytes.len() > 1048576 {
        return Err("payload".into());
    }
    let mut ring = vec![0; 1048592];
    let mut palette = vec![0; 768];
    let mut trans = vec![0; 256];
    let mut gamma_table = vec![0; 65536];
    let mut dict = vec![0; 32768];
    let mut b = Buffers {
        ring: &mut ring,
        palette: &mut palette,
        transparency: &mut trans,
        gamma: &mut gamma_table,
        dictionary: &mut dict,
    };
    let mut d = Decoder::new();
    d.display_gamma = gamma as f64 / 1000.0;
    let mut inf = State::new(if bits == 0 {
        BitBuffer::Bits32
    } else {
        BitBuffer::Bits64
    });
    let mut trace = Trace {
        events: Vec::new(),
        stop,
        rows: 0,
    };
    let mut reads = Vec::new();
    let mut status = 0;
    if mode == 0 {
        let mut pos = 0;
        let mut end = step.min(bytes.len());
        while pos < bytes.len() {
            let result = d.feed(&bytes[pos..end], &mut b, &mut inf, &NativeMath, &mut trace);
            let r = result.as_ref().map(|v| *v as i64).unwrap_or(-1);
            reads.push(format!("F:{pos}:{}:{r}", end - pos));
            match result {
                Ok(n) => {
                    pos += n;
                    if pos == end || n == 0 {
                        if end == bytes.len() {
                            break;
                        }
                        end = (end + step).min(bytes.len());
                    }
                }
                Err(_) => {
                    status = 8;
                    break;
                }
            }
        }
    } else {
        let mut input = Input {
            bytes: &bytes,
            at: 0,
            step,
            fail,
            events: Vec::new(),
        };
        let mut src = vec![0; sw];
        let mut dst = vec![0; dw];
        let mut sums = vec![0; dw];
        let mut counts = vec![0; dw];
        let mut read = vec![0; 4096];
        status = render_png(
            &mut input,
            sw as u16,
            sh as u16,
            dw as u16,
            dh as u16,
            &mut d,
            &mut inf,
            &mut b,
            RenderBuffers {
                src: &mut src,
                read: &mut read,
                dst: &mut dst,
                sums: &mut sums,
                counts: &mut counts,
            },
            &NativeMath,
            &mut trace,
        )
        .map_err(|_| "safety")? as u8;
        reads = input.events;
    }
    print!(
        "PN1 {status} {} {} {}",
        reads.len(),
        trace.events.len(),
        if mode == 0 {
            hex(&d.background)
        } else {
            "000000".into()
        }
    );
    for event in reads.into_iter().chain(trace.events) {
        print!(" {event}");
    }
    println!();
    Ok(())
}
