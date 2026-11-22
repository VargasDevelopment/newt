//! Bind the release header and generate What's new from whats_new_notes.h.
use std::{env, fmt::Write, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let header = "../firmware/native/firmware_version.h";
    println!("cargo:rerun-if-changed={header}");
    let source = fs::read_to_string(header)?;
    let versions: Vec<_> = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix("#define NEWT_FIRMWARE_VERSION \"")?.strip_suffix('"')
        })
        .collect();
    let [version] = versions.as_slice() else {
        return Err("one firmware version required".into());
    };
    if version.is_empty()
        || !version.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
    {
        return Err("invalid firmware version".into());
    }
    if env::var("NEWT_FIRMWARE_VERSION").is_ok_and(|value| value != *version) {
        return Err("image identity differs from firmware version header".into());
    }
    println!("cargo:rustc-env=NEWT_FIRMWARE_VERSION={version}");
    println!("cargo:rerun-if-changed=whats_new_notes.h");
    let source = fs::read_to_string("whats_new_notes.h")?;
    let body = source.split_once("kWhatsNew[] = {").ok_or("notes array")?.1;
    let body = body.split_once("};").ok_or("notes array end")?.0;
    let mut updates = Vec::<Vec<&str>>::new();
    let mut current = Vec::new();
    for line in body.lines().map(str::trim) {
        if let (Some(first), Some(last)) = (line.find('"'), line.rfind('"')) {
            assert!(last > first, "one complete quoted note per line");
            current.push(&line[first..=last]);
        }
        if line.ends_with("}},") {
            assert!(
                (2..=7).contains(&current.len()),
                "date and one to six changes"
            );
            updates.push(std::mem::take(&mut current));
        }
    }
    assert!(
        current.is_empty() && !updates.is_empty(),
        "complete update records"
    );
    let mut generated = format!(
        "pub static WHATS_NEW: [WhatsNewUpdate<'static>; {}] = [\n",
        updates.len()
    );
    for update in updates {
        writeln!(
            generated,
            "WhatsNewUpdate {{ date: Some({}.as_bytes()), changes: [",
            update[0]
        )?;
        for change in &update[1..] {
            writeln!(generated, "Some({change}.as_bytes()),")?;
        }
        for _ in update.len() - 1..6 {
            generated.push_str("None,\n");
        }
        generated.push_str("] },\n");
    }
    generated.push_str("];\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo output directory")?);
    fs::write(out.join("whats_new.rs"), generated)?;
    Ok(())
}
