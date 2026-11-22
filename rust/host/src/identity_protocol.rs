//! Test-only addition to host/src/main.rs; host allocation stays outside core.
use super::{Memory, encoded, hex, number, progress_value};
use newt_core::durable::{Codec, Commit, Load};
use newt_core::{library_name, sd_resume::SdResumeRecord, selected_book, sleep_intent};

// Explicit protocol projection of C++ primitive fields, never Rust object bytes.
fn raw_record(word: &str) -> SdResumeRecord {
    let bytes = hex(word);
    assert_eq!(bytes.len(), 140);
    let words = core::array::from_fn(|index| {
        let at = index * 4;
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    });
    SdResumeRecord::from_words(&words)
}

fn raw_text(record: SdResumeRecord) -> String {
    let mut bytes = [0; 140];
    let mut words = [0; newt_core::sd_resume::RETAINED_WORDS];
    record.write_words(&mut words);
    for (out, word) in bytes.chunks_exact_mut(4).zip(words) {
        out.copy_from_slice(&word.to_le_bytes());
    }
    encoded(&bytes)
}

fn status<V>(load: Load<V>, text: impl FnOnce(V) -> String) -> String {
    match load {
        Load::Loaded(value) => format!("loaded:{}", text(value)),
        Load::Empty => "empty".into(),
        Load::Ambiguous => "ambiguous".into(),
        Load::Failed => "failed".into(),
    }
}

fn commit(result: Commit) -> String {
    match result {
        Commit::Saved => "saved".into(),
        Commit::Unconfirmed => "unconfirmed".into(),
    }
}

fn intent(fields: &[&str]) -> sleep_intent::SleepResumeIntent {
    sleep_intent::SleepResumeIntent {
        book: number(fields[0]),
        checkpoint: number(fields[1]) as u32,
        settings: number(fields[2]) as u32,
    }
}

fn intent_text(value: sleep_intent::SleepResumeIntent) -> String {
    format!("{}:{}:{}", value.book, value.checkpoint, value.settings)
}

fn store(fields: &[&str]) -> String {
    let adjusted = ["zone", "1", fields[1], fields[2], fields[3], fields[4]];
    let mut memory = Memory::from_fields(&adjusted);
    let result = if fields[0] == "selected_store" {
        let mut store = selected_book::SelectedBookStore::new(&mut memory);
        match fields[5] {
            "load" => status(store.load(), raw_text),
            "book" => status(store.load_book(number(fields[7])), raw_text),
            "save" => commit(store.save(raw_record(fields[6]))),
            "protected" => commit(store.save_protected(raw_record(fields[6]), number(fields[7]))),
            _ => panic!("unknown selected operation"),
        }
    } else {
        let mut store = sleep_intent::SleepResumeStore::new(&mut memory);
        match fields[5] {
            "load" => status(store.load(), intent_text),
            "save" => commit(store.save(intent(&fields[6..]))),
            _ => panic!("unknown sleep operation"),
        }
    };
    memory.state(&result)
}

pub(super) fn run(fields: &[&str]) -> Option<String> {
    Some(match fields[0] {
        "name_valid" => {
            u8::from(library_name::valid_library_book_name(&hex(fields[1]))).to_string()
        }
        "sd_make" => {
            let name = hex(fields[1]);
            let digest = hex(fields[2]).try_into().unwrap();
            core::ffi::CStr::from_bytes_until_nul(&name)
                .ok()
                .and_then(|name| SdResumeRecord::new(name, digest))
                .map_or("bad".into(), |value| format!("ok {}", raw_text(value)))
        }
        "sd_validate" => u8::from(raw_record(fields[1]).valid()).to_string(),
        "selected_encode" => {
            let mut bytes = [0; selected_book::BYTES];
            if selected_book::SelectedBookCodec::encode(
                raw_record(fields[1]),
                number(fields[2]) as u32,
                &mut bytes,
            ) {
                encoded(&bytes)
            } else {
                "-".into()
            }
        }
        "selected_decode" => selected_book::SelectedBookCodec::decode(&hex(fields[1]))
            .map_or("bad".into(), |value| format!("ok {}", raw_text(value))),
        "sleep_encode" => {
            let mut bytes = [0; sleep_intent::BYTES];
            if sleep_intent::SleepResumeCodec::encode(
                intent(&fields[1..]),
                number(fields[4]) as u32,
                &mut bytes,
            ) {
                encoded(&bytes)
            } else {
                "-".into()
            }
        }
        "sleep_decode" => sleep_intent::SleepResumeCodec::decode(&hex(fields[1]))
            .map_or("bad".into(), |value| format!("ok {}", intent_text(value))),
        "sleep_checkpoint" => {
            sleep_intent::progress_checkpoint(&progress_value(&fields[1..])).to_string()
        }
        "selected_store" | "sleep_store" => store(fields),
        _ => return None,
    })
}
