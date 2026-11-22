use newt_core::hyphenator::Hyphenator;
use std::io::{self, Read};

enum Command {
    Init(Option<Vec<u8>>),
    Break(usize, Vec<u8>),
    Ready,
}
fn hex(s: &str) -> Result<Vec<u8>, &'static str> {
    if s == "-" {
        return Ok(Vec::new());
    }
    if s.len() % 2 != 0 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("hex");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "hex"))
        .collect()
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut commands = Vec::new();
    for line in input.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        commands.push(match fields.as_slice() {
            ["I", "null"] => Command::Init(None),
            ["I", data] => Command::Init(Some(hex(data)?)),
            ["B", cap, word] => {
                if cap.is_empty() || !cap.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("cap".into());
                }
                let cap: usize = cap.parse()?;
                if cap > 255 {
                    return Err("cap".into());
                }
                Command::Break(cap, hex(word)?)
            }
            ["R"] => Command::Ready,
            _ => return Err("command".into()),
        });
    }
    // All backing allocations precede borrowing; repeated init exercises actual state.
    let mut h = Hyphenator::new();
    for command in &commands {
        match command {
            Command::Init(data) => println!(
                "I {} {}",
                u8::from(h.init(data.as_deref())),
                u8::from(h.ready())
            ),
            Command::Ready => println!("R {}", u8::from(h.ready())),
            Command::Break(cap, word) => {
                let mut output = [0xa5; 255];
                let n = h.break_positions(word, &mut output[..*cap]);
                if output[n..].iter().any(|b| *b != 0xa5) {
                    return Err("output tail changed".into());
                }
                print!("B {n}");
                for value in &output[..n] {
                    print!(" {value}");
                }
                println!();
            }
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
