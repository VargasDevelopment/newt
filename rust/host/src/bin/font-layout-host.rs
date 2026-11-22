#![forbid(unsafe_code)]
use newt_core::font_layout::{ligature_glyph_id, pair_kern_adjustment};
use std::io::{self, Read};
fn number(s: &str) -> Result<u32, ()> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(());
    }
    s.parse().map_err(|_| ())
}
fn run() -> Result<(), ()> {
    let mut raw = Vec::new();
    io::stdin()
        .take(8_388_609)
        .read_to_end(&mut raw)
        .map_err(|_| ())?;
    if raw.len() > 8_388_608 || !raw.is_ascii() || raw.contains(&b'\r') {
        return Err(());
    }
    let raw = std::str::from_utf8(&raw).map_err(|_| ())?;
    let rows: Vec<_> = raw.split('\n').collect();
    if rows.len() != 4 || rows[3] != "" {
        return Err(());
    }
    let mode = rows[0];
    if mode != "P" && mode != "S" {
        return Err(());
    }
    let hex = rows[1];
    let mut table = Vec::new();
    if hex != "-" {
        if hex.is_empty()
            || hex.len() % 2 != 0
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(());
        }
        for chunk in hex.as_bytes().chunks_exact(2) {
            table.push(
                u8::from_str_radix(std::str::from_utf8(chunk).map_err(|_| ())?, 16)
                    .map_err(|_| ())?,
            );
        }
    }
    let glyphs: Vec<u32> = rows[2].split(' ').map(number).collect::<Result<_, _>>()?;
    let result = if mode == "P" {
        if glyphs.len() != 2 {
            return Err(());
        }
        pair_kern_adjustment(&table, glyphs[0], glyphs[1])
    } else {
        // Length is inferred from the actual borrowed sequence, including unsupported lengths.
        if glyphs.len() > 16 {
            return Err(());
        }
        ligature_glyph_id(&table, &glyphs) as i32
    };
    println!("R {result}");
    Ok(())
}
fn main() {
    if run().is_err() {
        std::process::exit(2);
    }
}
