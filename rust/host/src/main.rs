//! Test-only line protocol. Allocation and std belong to the host, not the core.
mod admission_protocol;
mod stats_service_protocol;
mod external_progress_protocol;
mod clock_protocol;
mod host_catalog;
mod host_frame;
mod host_stats;
mod persistence_protocol;
mod book_graph_protocol;
mod sleep_crossink_protocol;
mod host_tracker;
mod identity_protocol;
use newt_core::{
    durable::*, preferences::*, preferences_store, progress, progress_store, refresh, sd_record,
};
use std::io::{self, BufRead, Write};

fn number(word: &str) -> u64 {
    word.parse().expect("numeric protocol field")
}
fn hex(word: &str) -> Vec<u8> {
    if word == "-" || word == "_" {
        return Vec::new();
    }
    assert!(word.len().is_multiple_of(2));
    word.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).expect("hex protocol field")
        })
        .collect()
}
fn encoded(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".into();
    }
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(DIGITS[usize::from(byte >> 4)] as char);
        output.push(DIGITS[usize::from(byte & 15)] as char);
    }
    output
}
fn progress_value(fields: &[&str]) -> progress::Progress {
    progress::Progress {
        sequence: number(fields[0]) as u32,
        book: number(fields[1]),
        locator: progress::Locator {
            spine: number(fields[2]) as u32,
            character: number(fields[3]) as u32,
        },
        active_milliseconds: number(fields[4]),
        forward_pages: number(fields[5]) as u32,
        qualified_sessions: number(fields[6]) as u32,
        session_milliseconds: number(fields[7]) as u32,
    }
}
fn progress_text(value: progress::Progress) -> String {
    format!(
        "{} {} {} {} {} {} {} {}",
        value.sequence,
        value.book,
        value.locator.spine,
        value.locator.character,
        value.active_milliseconds,
        value.forward_pages,
        value.qualified_sessions,
        value.session_milliseconds
    )
}

struct Memory {
    visible: [Vec<u8>; 2],
    persisted: [Vec<u8>; 2],
    present: [bool; 2],
    persisted_present: [bool; 2],
    exists: bool,
    dirty: bool,
    mask: u32,
    script: Vec<u8>,
    namespace: String,
    trace: String,
    begins: u32,
    ends: u32,
    reads: u32,
    writes: u32,
    target: usize,
}
impl Memory {
    fn from_fields(fields: &[&str]) -> Self {
        let mask = number(fields[2]) as u32;
        let visible = [hex(fields[4]), hex(fields[5])];
        let present = [fields[4] != "-", fields[5] != "-"];
        Self {
            persisted: visible.clone(),
            visible,
            persisted_present: present,
            present,
            exists: mask & 2 == 0,
            dirty: mask & 128 != 0,
            mask,
            script: fields[3].as_bytes().to_vec(),
            namespace: "-".into(),
            trace: String::new(),
            begins: 0,
            ends: 0,
            reads: 0,
            writes: 0,
            target: 2,
        }
    }
    fn state(&self, result: &str) -> String {
        format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {}",
            result,
            self.begins,
            self.ends,
            self.reads,
            self.writes,
            self.target,
            u8::from(!self.dirty),
            self.namespace,
            if self.trace.is_empty() {
                "-"
            } else {
                &self.trace
            },
            encoded(&self.visible[0]),
            encoded(&self.visible[1]),
            encoded(&self.persisted[0]),
            encoded(&self.persisted[1])
        )
    }
}
fn index(key: &str) -> usize {
    match key {
        "slot0" => 0,
        "slot1" => 1,
        _ => panic!("unexpected slot key"),
    }
}
impl Backend for Memory {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        self.begins += 1;
        self.namespace = namespace.into();
        self.trace.push(if readonly { 'R' } else { 'W' });
        if self.mask & 1 != 0 {
            return StorageOpen::Failed;
        }
        if readonly && !self.exists {
            return StorageOpen::Missing;
        }
        self.exists = true;
        StorageOpen::Opened
    }
    fn end(&mut self) {
        self.ends += 1;
        self.trace.push('E');
    }
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        self.reads += 1;
        let i = index(key);
        self.trace.push(if i == 0 { '0' } else { '1' });
        if self.mask & (4 << i) != 0 || (self.writes != 0 && self.mask & 16 != 0) {
            return BlobRead::Failed;
        }
        if !self.present[i] {
            return BlobRead::Missing;
        }
        let mut observed = self.visible[i].len();
        if self.writes != 0 && self.mask & 32 != 0 {
            observed = observed.saturating_sub(1);
        }
        if self.mask & 512 != 0 && output.len() < 24 {
            observed = observed.saturating_add(1);
        }
        if self.mask & 256 == 0 || observed == output.len() {
            let count = self.visible[i].len().min(output.len());
            output[..count].copy_from_slice(&self.visible[i][..count]);
        }
        if self.writes != 0 && self.mask & 64 != 0 && !output.is_empty() {
            output[0] ^= 1;
        }
        BlobRead::Present(observed)
    }
    fn put_bytes(&mut self, _: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        let fault = self
            .script
            .get(self.writes as usize)
            .copied()
            .unwrap_or(b'g');
        self.writes += 1;
        self.target = index(key);
        self.trace.push(if self.target == 0 { 'A' } else { 'B' });
        self.visible[self.target] = bytes.to_vec();
        self.present[self.target] = true;
        if fault == b't' {
            self.visible[self.target][12] ^= 1;
        }
        if fault == b'b' {
            self.dirty = true;
        } else {
            self.persisted = self.visible.clone();
            self.persisted_present = self.present;
            self.dirty = false;
        }
        match fault {
            b'a' | b'b' => 0,
            b't' => bytes.len() / 2,
            _ => bytes.len(),
        }
    }
    fn readback_durable(&self) -> bool {
        !self.dirty
    }
}

struct Note<C>(std::marker::PhantomData<C>);
impl<C: Codec<20, Value = u32>> Codec<20> for Note<C> {
    type Value = u32;
    const NAMESPACE: &'static str = C::NAMESPACE;
    const REPLACE_DAMAGED: bool = true;
    fn known(bytes: &[u8]) -> bool {
        C::known(bytes)
    }
    fn decode(bytes: &[u8]) -> Option<u32> {
        C::decode(bytes)
    }
    fn encode(value: u32, sequence: u32, bytes: &mut [u8; 20]) -> bool {
        C::encode(value, sequence, bytes)
    }
}
fn load_text(value: Load<u32>) -> String {
    match value {
        Load::Loaded(v) => format!("loaded:{v}"),
        Load::Empty => "empty".into(),
        Load::Ambiguous => "ambiguous".into(),
        Load::Failed => "failed".into(),
    }
}
fn store<C: Codec<20, Value = u32>, const ALLOW: bool>(fields: &[&str]) -> String {
    let mut memory = Memory::from_fields(fields);
    let mut records = RecordStore::<_, C, 20, ALLOW>::new(&mut memory);
    let value = number(fields[7]) as u32;
    let protected = number(fields[8]) as u32;
    let result = match fields[6] {
        "load" => load_text(records.load()),
        "matching" => load_text(records.load_matching(|v| v % 2 == value % 2, |a, b| a == b)),
        "save" | "protected" | "admitted" => {
            let result = if fields[6] == "protected" {
                records.save_protected(value, protected)
            } else if fields[6] == "admitted" {
                let mut quiet = || protected != 0;
                let mut admission = Admission {
                    quiet: Some(&mut quiet),
                    entered: false,
                    deferred: false,
                };
                records.save_admitted(value, &mut admission)
            } else {
                records.save(value)
            };
            match result {
                Commit::Saved => "saved".into(),
                Commit::Unconfirmed => "unconfirmed".into(),
            }
        }
        "publish" => {
            let mut live = protected;
            let result = commit_publish::<_, C, 20>(&mut memory, value, &mut live);
            format!(
                "{}:{live}",
                if result == Commit::Saved {
                    "saved"
                } else {
                    "unconfirmed"
                }
            )
        }
        "restore" => format!("loaded:{}", restore::<_, C, 20>(&mut memory, 0)),
        _ => panic!("unknown store operation"),
    };
    memory.state(&result)
}
fn scalar<C: Codec<20, Value = u32>>(fields: &[&str]) -> String {
    if fields[0] == "scalar_encode" {
        let mut bytes = [0; 20];
        if C::encode(
            number(fields[2]) as u32,
            number(fields[3]) as u32,
            &mut bytes,
        ) {
            encoded(&bytes)
        } else {
            "-".into()
        }
    } else {
        C::decode(&hex(fields[2])).map_or("bad".into(), |value| format!("ok {value}"))
    }
}
fn scalar_dispatch(fields: &[&str]) -> String {
    match fields[1] {
        "clock" => scalar::<ClockFormatCodec>(fields),
        "zone" => scalar::<TimeZoneCodec>(fields),
        "sleep" => scalar::<SleepScreenCodec>(fields),
        _ => panic!("unknown scalar codec"),
    }
}
fn store_dispatch<const ALLOW: bool>(fields: &[&str]) -> String {
    match fields[0] {
        "zone" => store::<TimeZoneCodec, ALLOW>(fields),
        "note" => store::<Note<TimeZoneCodec>, ALLOW>(fields),
        _ => panic!("unknown store codec"),
    }
}

fn progress_store_command(fields: &[&str]) -> String {
    let adjusted = ["zone", "1", fields[1], fields[2], fields[3], fields[4]];
    let mut memory = Memory::from_fields(&adjusted);
    let mut store = progress_store::ProgressStore::new(&mut memory);
    let mut value = progress_value(&fields[8..]);
    let mut peers = progress_store::PeerLocators {
        present: 128,
        slots: [
            progress::Locator {
                spine: 99,
                character: 98,
            },
            progress::Locator {
                spine: 97,
                character: 96,
            },
        ],
    };
    let protected = if fields[6] == "-" {
        None
    } else {
        Some(number(fields[6]) as u32)
    };
    let result = match fields[5] {
        "load" | "checkpoint" => {
            let loaded = if fields[5] == "load" {
                store.load()
            } else {
                store.load_checkpoint(protected.unwrap())
            };
            match loaded {
                Load::Loaded(loaded) => {
                    value = loaded;
                    "loaded"
                }
                Load::Empty => "empty",
                Load::Ambiguous => "ambiguous",
                Load::Failed => "failed",
            }
        }
        "peers" => match store.load_peer_locators(&mut peers) {
            Load::Loaded(()) => "loaded",
            Load::Empty => "empty",
            Load::Ambiguous => "ambiguous",
            Load::Failed => "failed",
        },
        "save" | "save_peers" => {
            let result = if fields[5] == "save_peers" {
                store.save_with_peers(&mut value, &mut peers, protected, fields[7] == "1")
            } else {
                store.save(&mut value, protected, fields[7] == "1")
            };
            if result == Commit::Saved {
                "saved"
            } else {
                "unconfirmed"
            }
        }
        _ => panic!("unknown progress store operation"),
    };
    memory.state(&format!(
        "{} {} {} {} {} {} {}",
        result,
        progress_text(value),
        peers.present,
        peers.slots[0].spine,
        peers.slots[0].character,
        peers.slots[1].spine,
        peers.slots[1].character
    ))
}

fn preferences_store_command(fields: &[&str]) -> String {
    let adjusted = ["zone", "1", fields[1], fields[2], fields[3], fields[4]];
    let mut memory = Memory::from_fields(&adjusted);
    let mut value = Preferences {
        sequence: number(fields[6]) as u32,
        settings: ViewSettings {
            orientation: match number(fields[7]) {
                0 => Orientation::Portrait,
                1 => Orientation::LandscapeCcw,
                _ => panic!("protocol proposes only representable orientations"),
            },
            slim_progress_bar: number(fields[8]) != 0,
            refresh_pages: number(fields[9]) as u8,
            // The C++ parity protocol predates text sizes; it speaks only 30 px.
            text_size: TextSize::Px30,
        },
    };
    let mut store = preferences_store::PreferencesStore::new(&mut memory);
    let result = match fields[5] {
        "load" => store.load().map(|loaded| {
            value = loaded;
        }),
        "save" => store.save(&mut value),
        _ => panic!("unknown preferences store command"),
    };
    let status = match result {
        Ok(()) => "loaded",
        Err(preferences_store::StoreError::Empty) => "empty",
        Err(preferences_store::StoreError::Corrupt) => "corrupt",
        Err(preferences_store::StoreError::Unsupported) => "unsupported",
        Err(preferences_store::StoreError::Ambiguous) => "ambiguous",
        Err(preferences_store::StoreError::Failed) => "failed",
    };
    memory.state(&format!(
        "{} {} {} {} {}",
        status,
        value.sequence,
        value.settings.orientation as u8,
        u8::from(value.settings.slim_progress_bar),
        value.settings.refresh_pages
    ))
}

fn run(fields: &[&str]) -> String {
    if fields[0]=="stats_service" { return encoded(stats_service_protocol::run(std::str::from_utf8(&hex(fields[1])).expect("UTF8 script")).as_bytes()); }
    if fields[0].starts_with("xp.") { return external_progress_protocol::run(&fields.join(" ")).expect("valid external-progress fixture"); }
    if fields[0]=="stats_persist" {return persistence_protocol::run(hex(fields[1]));}
    if fields[0] == "book_graph" {
        return encoded(&book_graph_protocol::nrust19::run(&hex(fields[1])).expect("book graph protocol"));
    }
    if fields[0].starts_with("sc_") { return sleep_crossink_protocol::run(fields); }
    if let Some(reply) = admission_protocol::run(fields) {
        return reply;
    }
    match fields[0] {
        "catalog" => return host_catalog::catalog_command(fields),
        "catalog_path" => return host_catalog::path_command(fields),
        _ => {}
    }
    match fields[0] {
        "stats_ledger" => return host_stats::ledger_command(fields),
        "stats_wire" => return host_stats::wire_command(fields),
        "stats_make" => return host_stats::make_command(fields),
        "stats_summary" => return host_stats::summary_command(fields),
        _ => {}
    }
    match fields[0] {
        "tracker" => return host_tracker::tracker_command(fields),
        "tracker_clock" => return host_tracker::clock_command(fields),
        _ => {}
    }
    if let Some(reply) = identity_protocol::run(fields) {
        return reply;
    }
    if let Some(result) = clock_protocol::command(fields) {
        return result;
    }
    if fields[0].starts_with("frame_") {
        return host_frame::command(fields);
    }
    match fields[0] {
        "progress_encode" => progress_value(&fields[1..])
            .encode()
            .map_or("-".into(), |bytes| encoded(&bytes)),
        "progress_decode" => progress::Progress::decode(&hex(fields[1]))
            .map_or("bad".into(), |v| format!("ok {}", progress_text(v))),
        "progress_select" => match progress::select(&hex(fields[1]), &hex(fields[2])) {
            progress::Selection::None => "none".into(),
            progress::Selection::Ambiguous => "ambiguous".into(),
            progress::Selection::First(v) => format!("first {}", progress_text(v)),
            progress::Selection::Second(v) => format!("second {}", progress_text(v)),
        },
        "progress_plan" => progress::plan(
            &hex(fields[1]),
            &hex(fields[2]),
            progress_value(&fields[3..]),
        )
        .map_or("bad".into(), |p| {
            format!("{} {}", p.target_slot, progress_text(p.value))
        }),
        "prefs_encode" => {
            let orientation = match number(fields[2]) {
                0 => Orientation::Portrait,
                1 => Orientation::LandscapeCcw,
                _ => return "-".into(),
            };
            Preferences {
                sequence: number(fields[1]) as u32,
                settings: ViewSettings {
                    orientation,
                    slim_progress_bar: number(fields[3]) != 0,
                    refresh_pages: number(fields[4]) as u8,
                    // The C++ parity protocol predates text sizes; it speaks only 30 px.
                    text_size: TextSize::Px30,
                },
            }
            .encode()
            .map_or("-".into(), |bytes| encoded(&bytes))
        }
        "prefs_decode" => match Preferences::decode(&hex(fields[1])) {
            Ok(v) => format!(
                "ok {} {} {} {}",
                v.sequence,
                v.settings.orientation as u8,
                u8::from(v.settings.slim_progress_bar),
                v.settings.refresh_pages
            ),
            Err(DecodeError::Corrupt) => "corrupt".into(),
            Err(DecodeError::Unsupported) => "unsupported".into(),
        },
        "geometry" => {
            let g = geometry(ViewSettings {
                orientation: if fields[1] == "0" {
                    Orientation::Portrait
                } else {
                    Orientation::LandscapeCcw
                },
                ..ViewSettings::default()
            });
            format!(
                "{} {} {} {} {} {}",
                g.width, g.height, g.margin, g.content_bottom, g.bar_y, g.footer_y
            )
        }
        "refresh" => {
            let mut reader = refresh::ReadingRefresh::default();
            let mut ui = refresh::UiRefresh::default();
            let mut output = String::new();
            for action in fields[2].bytes() {
                match action {
                    b'c' => reader.page_changed(),
                    b'f' => {
                        reader.painted(true);
                        ui.painted(true);
                    }
                    b'p' => {
                        reader.painted(false);
                        ui.painted(false);
                    }
                    b'r' => reader.reset(),
                    b'q' => {}
                    _ => panic!("bad action"),
                }
                output.push(if reader.needs_full(number(fields[1]) as u32) {
                    '1'
                } else {
                    '0'
                });
                output.push(if ui.needs_full() { '1' } else { '0' });
            }
            output
        }
        "sd_seal" => {
            let mut record: [u8; 256] = hex(fields[5]).try_into().expect("record size");
            let tag: [u8; 4] = hex(fields[1]).try_into().expect("tag size");
            let result = sd_record::seal(
                &mut record,
                &tag,
                &hex(fields[2]),
                number(fields[3]) as usize,
                number(fields[4]) as u32,
            );
            format!("{} {}", u8::from(result), encoded(&record))
        }
        "sd_header" => {
            let mut record: [u8; 256] = hex(fields[2]).try_into().expect("record size");
            let result = sd_record::header(&mut record, &hex(fields[1]));
            format!("{} {}", u8::from(result), encoded(&record))
        }
        "sd_intact" => {
            let record: [u8; 256] = hex(fields[2]).try_into().expect("record size");
            let tag: [u8; 4] = hex(fields[1]).try_into().expect("tag size");
            u8::from(sd_record::intact(&record, &tag)).to_string()
        }
        "scalar_encode" | "scalar_decode" => scalar_dispatch(fields),
        "store" => {
            if fields[2] == "1" {
                store_dispatch::<true>(&fields[1..])
            } else {
                store_dispatch::<false>(&fields[1..])
            }
        }
        "progress_store" => progress_store_command(fields),
        "preferences_store" => preferences_store_command(fields),
        _ => panic!("unknown command"),
    }
}

fn main() {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line.expect("stdin");
        let fields: Vec<_> = line.split_whitespace().collect();
        assert!(!fields.is_empty(), "empty protocol command");
        writeln!(output, "{}", run(&fields)).expect("stdout");
    }
}

#[cfg(test)]
mod probe_regressions;
