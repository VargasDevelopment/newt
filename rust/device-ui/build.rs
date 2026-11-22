//! Generate What's new from whats_new_notes.h.
use std::{env, fmt::Write, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
