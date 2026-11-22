#[path = "../xml_session/expat.rs"]
mod expat;
#[cfg(test)]
#[path = "../xml_session/native_tests.rs"]
mod native_tests;
use newt_core::xml_session::*;
use newt_core::{zip_container::Entry, zip_window::source::BookSource};
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
struct Sink {
    stop: bool,
    stop_on_text: bool,
}
impl Handler for Sink {
    fn start_element(&mut self, name: &[u8], attributes: &mut dyn Attributes) {
        print!("S {}", hex(name));
        while let Some((name, value)) = attributes.next() {
            print!(" {} {}", hex(name), hex(value));
        }
        println!();
    }
    fn end_element(&mut self, name: &[u8]) {
        println!("E {}", hex(name));
    }
    fn text(&mut self, bytes: &[u8]) {
        println!("T {}", hex(bytes));
        if self.stop_on_text {
            self.stop = true;
        }
    }
    fn stop_parse(&self) -> bool {
        self.stop
    }
}
struct Policy {
    cancel: i64,
    polls: i64,
}
impl ProcessingPolicy for Policy {
    fn cancelled(&mut self) -> bool {
        let poll = self.polls;
        self.polls += 1;
        let cancel = poll == self.cancel;
        println!("Q {poll} {}", u8::from(cancel));
        cancel
    }
}
fn run(line: &str) -> Result<(), ()> {
    let p: Vec<_> = line.split(' ').collect();
    if !((p.len() == 7 && p[0] == "X") || (p.len() == 12 && p[0] == "Y")) {
        return Err(());
    }
    let bytes = decode(p[1])?;
    if bytes.len() > 1048576 {
        return Err(());
    }
    let number = |s: &str| -> Result<usize, ()> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(());
        }
        let n = s.parse::<usize>().map_err(|_| ())?;
        if n > 1048576 { Err(()) } else { Ok(n) }
    };
    let max = number(p[2])?;
    if max == 0 {
        return Err(());
    }
    let fail = if p[3] == "-1" {
        -1
    } else {
        number(p[3])? as i64
    };
    let filtering = match p[4] {
        "0" => false,
        "1" => true,
        _ => return Err(()),
    };
    let mut sink = match p[5] {
        "0" => Sink {
            stop: false,
            stop_on_text: false,
        },
        "1" => Sink {
            stop: true,
            stop_on_text: false,
        },
        "2" => Sink {
            stop: false,
            stop_on_text: true,
        },
        _ => return Err(()),
    };
    let extended = p[0] == "Y";
    let producer = extended && p[6] == "1";
    if extended && !matches!(p[6], "0" | "1") {
        return Err(());
    }
    let cancel = if !extended || p[7] == "-1" {
        -1
    } else {
        number(p[7])? as i64
    };
    let method = if extended { number(p[8])? as u16 } else { 0 };
    let local = extended && p[9] == "1";
    if extended && (!matches!(p[8], "0" | "8") || !matches!(p[9], "0" | "1")) {
        return Err(());
    }
    let size = if extended {
        number(p[10])?
    } else {
        bytes.len()
    };
    let action = p[if extended { 11 } else { 6 }];
    let actions: Vec<_> = action.split(',').collect();
    if action != "p" && (actions.is_empty() || actions.iter().any(|a| !matches!(*a, "f" | "c"))) {
        return Err(());
    }
    if local && bytes.len() < 30 {
        return Err(());
    }
    let mut entry = Entry::raw(bytes.len() as u32);
    entry.method = method;
    entry.uncompressed_size = size as u32;
    if local {
        entry.local_header_offset = 0;
        entry.compressed_size -= 30;
    }
    let mut source = Source {
        bytes,
        max,
        fail,
        calls: 0,
    };
    let mut backend = expat::Expat::default();
    let mut buffer = [0; 2048];
    let mut filter = newt_core::xml_input::EntityFilter::new();
    let resources = Resources {
        buffer: Some(&mut buffer),
        filter: Some(&mut filter),
    };
    let mut state = Box::new(newt_core::inflate::State::new(
        newt_core::inflate::BitBuffer::Bits32,
    ));
    let mut dictionary = [0; newt_core::inflate::DICTIONARY_SIZE];
    let mut input = [0; newt_core::zip_container::INPUT_SIZE];
    let decoder = local.then_some(newt_core::zip_container::InflateScratch {
        state: &mut state,
        dictionary: &mut dictionary,
        input: &mut input,
    });
    if producer {
        let mut policy = Policy { cancel, polls: 0 };
        execute(
            &mut source,
            &entry,
            decoder,
            &mut backend,
            &mut sink,
            resources,
            filtering,
            action,
            actions,
            BorrowedPolicy(&mut policy),
        );
    } else {
        execute(
            &mut source,
            &entry,
            decoder,
            &mut backend,
            &mut sink,
            resources,
            filtering,
            action,
            actions,
            NoPolicy,
        );
    }
    println!("END");
    Ok(())
}
fn execute<'a, P: SessionPolicy<expat::Expat>>(
    source: &'a mut Source,
    entry: &'a Entry,
    decoder: Option<newt_core::zip_container::InflateScratch<'a>>,
    backend: &'a mut expat::Expat,
    sink: &'a mut Sink,
    resources: Resources<'a>,
    filtering: bool,
    action: &str,
    actions: Vec<&str>,
    policy: P,
) {
    if action == "p" {
        let status = parse_entry_with_policy(
            source, entry, decoder, backend, sink, resources, filtering, policy,
        );
        println!("P {}", status.name());
    } else {
        let mut session = XmlSaxSession::with_policy(source, decoder, backend, policy);
        let status = session.open(entry, sink, resources, filtering);
        println!(
            "O {} {} {}",
            status.name(),
            session.bytes_consumed(),
            u8::from(session.is_open())
        );
        for action in actions {
            if action == "c" {
                session.close();
                println!("C {}", session.bytes_consumed());
            } else {
                let mut end = true;
                let status = session.feed_chunk(Some(&mut end));
                println!(
                    "F {} {} {} {}",
                    status.name(),
                    session.bytes_consumed(),
                    u8::from(end),
                    u8::from(session.is_open())
                );
            }
        }
    }
}
fn main() {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err()
        || !input.ends_with('\n')
        || input.contains('\r')
    {
        std::process::exit(2);
    }
    for line in input[..input.len() - 1].split('\n') {
        if run(line).is_err() {
            std::process::exit(2);
        }
    }
}
