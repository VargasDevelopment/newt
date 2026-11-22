//! Host-only transport namespace; not part of the no_std foundation.
use newt_staged_zip_window::{arena::Arena, source::BookSource, window::ZipDirectoryWindow};
use std::io::{self, Read};
struct TransportSource {
    physical: u64,
    advertised: u64,
    cursor: u64,
    reads: u64,
    limit: usize,
    probe: i32,
    fail_after: u64,
    oversized: bool,
    yield_action: u8,
    yield_on: u64,
    source_action: u8,
    source_on: u64,
    pattern: u8,
    yields: u64,
    live: bool,
}
impl TransportSource {
    fn act(&mut self, action: u8) {
        match action {
            1 => self.live = false,
            2 => self.advertised += 1,
            3 => self.probe = 1,
            _ => {}
        }
    }
    fn yielding(&mut self) {
        println!("Y");
        self.yields += 1;
        if self.yields == self.yield_on {
            self.act(self.yield_action);
        }
    }
}
impl BookSource for TransportSource {
    fn size(&self) -> u64 {
        println!("S {}", self.advertised);
        self.advertised
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if out.is_empty() {
            let got = if self.live { self.probe } else { -1 };
            println!("P {at} {got}");
            return got;
        }
        self.reads += 1;
        let got = if !self.live {
            -1
        } else if self.reads > self.fail_after || at >= self.physical {
            0
        } else if self.oversized {
            out.len() as i32 + 1
        } else {
            let got = out.len().min(self.limit).min((self.physical - at) as usize);
            for (i, b) in out[..got].iter_mut().enumerate() {
                let n = at + i as u64;
                *b = match self.pattern {
                    0 => ((n * 17 + n / 256) % 251) as u8,
                    1 => (n ^ (n >> 8)) as u8,
                    _ => 0,
                };
            }
            self.cursor = at + got as u64;
            got as i32
        };
        println!("R {at} {} {got}", out.len());
        if self.reads == self.source_on {
            self.act(self.source_action);
        }
        got
    }
}
fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let mut words = input.split_whitespace();
    let mut next = || {
        words
            .next()
            .expect("header")
            .parse::<u64>()
            .expect("integer")
    };
    let physical = next();
    let advertised = next();
    let capacity = next() as usize;
    let prefix = next() as usize;
    let limit = next() as usize;
    // Probe may be negative; parse it separately after ending this borrow.
    drop(next);
    let probe = words.next().unwrap().parse::<i32>().unwrap();
    let mut next = || words.next().unwrap().parse::<u64>().unwrap();
    let fail_after = next();
    let oversized = next() != 0;
    let yield_action = next() as u8;
    let yield_on = next();
    let source_action = next() as u8;
    let source_on = next();
    let pattern = next() as u8;
    let begin = next();
    let end = next();
    drop(next);
    assert!(
        capacity <= 65536
            && limit <= 16384
            && physical <= 1000000
            && advertised <= 1000000
            && pattern <= 2
    );
    let mut source = TransportSource {
        physical,
        advertised,
        cursor: 0,
        reads: 0,
        limit,
        probe,
        fail_after,
        oversized,
        yield_action,
        yield_on,
        source_action,
        source_on,
        pattern,
        yields: 0,
        live: true,
    };
    let mut storage = vec![0; capacity];
    let mut arena = Arena::new(&mut storage);
    arena.alloc(prefix, 1);
    {
        let mut window = ZipDirectoryWindow::new(
            &mut source,
            &mut arena,
            begin,
            end,
            Some(TransportSource::yielding),
        );
        println!(
            "A {} {} {}",
            window.arena_used(),
            window.arena_high_water(),
            window.arena_failed_alloc_size()
        );
        while let Some(command) = words.next() {
            assert_eq!(command, "R");
            let at = words.next().unwrap().parse().unwrap();
            let bytes = words.next().unwrap().parse::<u32>().unwrap();
            let present = words.next().unwrap().parse::<u8>().unwrap();
            assert!(bytes <= 16384 && present <= 1);
            let mut out = vec![0xa5; bytes as usize];
            let ok = window.read(at, if present == 1 { Some(&mut out) } else { None }, bytes);
            print!("O {} ", u8::from(ok));
            for b in out {
                print!("{b:02x}");
            }
            println!();
        }
    }
    println!(
        "D {} {} {} {}",
        arena.used(),
        arena.high_water(),
        arena.failed_alloc_size(),
        source.cursor
    );
}
