//! Host-only protocol glue, not part of no_std core.
use crate::{encoded, hex, number};
use newt_core::library_catalog::*;

fn input(word: &str) -> Option<Vec<u8>> {
    if word == "-" {
        return None;
    }
    let mut bytes = hex(word);
    bytes.push(0); // valid bounded host caller allocation, including corrupt bytes
    Some(bytes)
}
fn bare_bytes(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}
fn map_hex(map: &[u32; BOOK_CAPACITY]) -> String {
    encoded(&map.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>())
}
pub fn path_command(f: &[&str]) -> String {
    let name = input(f[3]);
    if f[1] == "B" {
        return library_base_name(name.as_deref()).map_or_else(|| "-".into(), encoded);
    }
    let directory = input(f[2]);
    let suffix = input(f[4]);
    let capacity = number(f[5]) as usize;
    assert!(capacity <= 256);
    let mut output = [0xa5; 256];
    let result = if f[1] == "P" {
        library_book_path(
            directory.as_deref(),
            name.as_deref(),
            if f[6] == "0" {
                None
            } else {
                Some(&mut output[..capacity])
            },
        )
    } else {
        library_name_file(
            directory.as_deref(),
            name.as_deref(),
            suffix.as_deref(),
            if f[6] == "0" {
                None
            } else {
                Some(&mut output[..capacity])
            },
        )
    };
    format!("{} {}", u8::from(result), encoded(&output))
}
/// catalog SCRIPT EVENT...; SCRIPT's 1/0 characters drive mode 5 proofs.
/// A,name,size,mode; O,N|F|S,count; R,status; F; Q; P,index,capacity.
pub fn catalog_command(f: &[&str]) -> String {
    let mut catalog = LibraryCatalog::default();
    let mut map = [0; BOOK_CAPACITY];
    let script = f[1].as_bytes();
    let mut at = 0;
    let mut rows = Vec::new();
    for event in &f[2..] {
        let e: Vec<_> = event.split(',').collect();
        let mut trace = Vec::new();
        let mut query = "- -".to_string();
        match e[0] {
            "A" => {
                let name = input(e[1]);
                let mode = number(e[3]);
                let mut same = |kept: &[u8], copy: &[u8]| {
                    let answer = match mode {
                        3 => true,
                        4 => {
                            let mut a = kept.to_vec();
                            a.push(0);
                            let mut b = copy.to_vec();
                            b.push(0);
                            library_base_name(Some(&a)) == library_base_name(Some(&b))
                        }
                        5 => {
                            let value = script.get(at) == Some(&b'1');
                            at += 1;
                            value
                        }
                        _ => false,
                    };
                    trace.push(format!(
                        "{}>{}={}",
                        encoded(kept),
                        encoded(copy),
                        u8::from(answer)
                    ));
                    answer
                };
                let mut copies = LibraryCopies {
                    bytes: map,
                    same: if mode >= 2 { Some(&mut same) } else { None },
                };
                for _ in 0..e.get(4).map_or(1, |value| number(value)) {
                    catalog.add(
                        name.as_deref().unwrap_or(&[]),
                        number(e[2]) as u32,
                        if mode == 0 { None } else { Some(&mut copies) },
                    );
                }
                map = copies.bytes;
            }
            "O" => {
                for _ in 0..number(e[2]) {
                    catalog.omit(match e[1] {
                        "N" => LibraryOmission::Name,
                        "F" => LibraryOmission::Folder,
                        _ => LibraryOmission::Stopped,
                    });
                }
            }
            "R" => catalog.reset(if e[1] == "0" {
                LibraryStatus::Ok
            } else {
                LibraryStatus::Unavailable
            }),
            "F" => {
                catalog.finish();
            }
            "Q" => {}
            "P" => {
                let mut output = [0xa5; 256];
                let capacity = number(e[2]) as usize;
                assert!(capacity <= output.len());
                let result = catalog.path(number(e[1]) as usize, Some(&mut output[..capacity]));
                query = format!("{} {}", u8::from(result), encoded(&output));
            }
            _ => panic!("catalog event"),
        }
        let o = catalog.omissions();
        let names: Vec<_> = (0..catalog.size())
            .map(|i| encoded(bare_bytes(catalog.filename(i).unwrap())))
            .collect();
        rows.push(format!(
            "{} {} {} {} {} {} {} {} {} {} {} {}",
            catalog.status() as u8,
            catalog.size(),
            catalog.copies(),
            o.full,
            o.names,
            o.folders,
            u8::from(o.stopped),
            catalog
                .notice()
                .map_or_else(|| "-".into(), |s| encoded(s.as_bytes())),
            if names.is_empty() {
                "-".into()
            } else {
                names.join("/")
            },
            map_hex(&map),
            if trace.is_empty() {
                "-".into()
            } else {
                trace.join("/")
            },
            query
        ));
    }
    rows.join(" ; ")
}
