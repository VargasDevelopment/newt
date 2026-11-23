// SPDX-License-Identifier: MIT
//! The bridge's card write (newt_core::dev_put) over a fake card and host:
//! the name and folder rules, admission, and that the final name only ever
//! holds the old file or the whole verified new one, whatever ends the lease.
mod fakes;
mod sha256;

use fakes::{Event, FakeCard, FakeWire, HostHash, Node, FAILED};
use newt_core::dev_put::{self, admit, name_allowed, Busy, End, Facts, Session, CAP_BYTES, CHUNK_BYTES, IDLE_MS,
    MEMORY_BYTES, RESERVE_BYTES};
use sha256::sha256;

const BOOT: u32 = 7;
const SESSION: u32 = 0x9000_0001;
const PUT_ID: u32 = 0x8000_0010;
const DEV: &str = "/newt-sd/Newt/Dev";
const TARGET: &str = "/newt-sd/Newt/Dev/runtime.bin";
const TEMP: &str = "/newt-sd/Newt/Dev/.newt-put";
const BANNER: &str = "PUT line=4160 chunk=3072 cap=7798784";

/// The host's side of one lease: lines with fresh IDs in the PUT's session.
struct Host { id: u32, script: Vec<Event> }
impl Host {
    fn new() -> Self { Self { id: PUT_ID, script: Vec::new() } }
    fn line(&mut self, body: &str) -> &mut Self {
        self.id += 1;
        self.script.push(Event::Line(format!("N1 {BOOT} {SESSION} {} {body}\n", self.id)));
        self
    }
    fn file(&mut self, folder: &str, name: &str, bytes: &[u8]) -> &mut Self {
        self.line(&format!("FILE {folder} {name} {} {}", bytes.len(), hex(&sha256(bytes))))
    }
    fn data(&mut self, bytes: &[u8]) -> &mut Self {
        for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            self.line(&format!("DATA {} {}", index * CHUNK_BYTES, base64(chunk)));
        }
        self
    }
    fn put(&mut self, folder: &str, name: &str, bytes: &[u8]) -> &mut Self {
        self.file(folder, name, bytes).data(bytes).line("DONE")
    }
    fn then(&mut self, event: Event) -> &mut Self {
        self.script.push(event);
        self
    }
}

struct Outcome { end: End, replies: Vec<(u32, String)>, wire: FakeWire }
impl Outcome {
    fn bodies(&self) -> Vec<&str> { self.replies.iter().map(|(_, body)| body.as_str()).collect() }
    fn last(&self) -> &str { &self.replies.last().expect("a reply").1 }
}

fn lease(card: &mut FakeCard, host: &mut Host) -> Outcome {
    lease_with(card, FakeWire::new(std::mem::take(&mut host.script)))
}

fn lease_with(card: &mut FakeCard, mut wire: FakeWire) -> Outcome {
    let mut memory: Box<[u8; MEMORY_BYTES]> = vec![0; MEMORY_BYTES].try_into().expect("lease memory");
    let session = Session { boot: BOOT, session: SESSION, request: PUT_ID };
    let end = dev_put::run(session, &mut memory, &mut wire, card, &mut HostHash::default());
    assert_eq!(card.open_files(), 0, "every exit closes its files");
    Outcome { end, replies: wire.replies(BOOT, SESSION), wire }
}

/// Bytes that differ at every offset in a chunk, so a misplaced chunk shows.
fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|i| (i * 7 + i / 251) as u8).collect()
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for group in bytes.chunks(3) {
        let bits = group.iter().enumerate().fold(0u32, |bits, (i, &b)| bits | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= group.len() { char::from(ALPHABET[(bits >> (18 - 6 * i) & 63) as usize]) } else { '=' });
        }
    }
    out
}

fn done(bytes: &[u8]) -> String { format!("DONE size={} sha256={}", bytes.len(), hex(&sha256(bytes))) }

/// After a lease that wrote nothing: no temp, and the target as it was.
fn unchanged(card: &FakeCard, old: Option<&[u8]>) {
    assert_eq!(card.file(TEMP), None, "the temp is removed");
    assert_eq!(card.file(TARGET), old, "the final name keeps what it held");
}

#[test]
fn names_outside_the_safe_set_are_refused() {
    for bad in [&b""[..], b"..", b"../x", b"a/b", b"/abs", b"a\\b", b".hidden", b"a..b", b"a b", b"x\0",
        "caf\u{e9}".as_bytes(), &[b'a'; 33]] {
        assert!(!name_allowed(bad), "{bad:?}");
    }
    for good in [&b"newt.bin"[..], b"app-1_x.txt", b"0", &[b'a'; 32]] {
        assert!(name_allowed(good), "{good:?}");
    }
    for name in ["..", "../Books", ".newt-put"] {
        let mut card = FakeCard::default();
        let outcome = lease(&mut card, Host::new().line(&format!("FILE dev {name} 0 {}", hex(&sha256(b"")))));
        assert_eq!(outcome.bodies(), [BANNER, "ERROR name"], "{name}");
        assert_eq!(card.nodes.len(), 1, "a refused name touches nothing");
    }
}

#[test]
fn only_the_three_dev_folders_are_targets() {
    for (folder, path) in [("apps", "/newt-sd/Newt/Apps/a.txt"), ("update", "/newt-sd/Newt/Update/a.txt"),
        ("dev", "/newt-sd/Newt/Dev/a.txt")] {
        let mut card = FakeCard::default();
        let outcome = lease(&mut card, Host::new().put(folder, "a.txt", b"hello"));
        assert_eq!(outcome.last(), done(b"hello"));
        assert_eq!(card.file(path), Some(&b"hello"[..]));
    }
    for folder in ["books", "Dev", "../Books", "/newt-sd/Newt/Dev"] {
        let mut card = FakeCard::default();
        let outcome = lease(&mut card, Host::new().file(folder, "a.txt", b"hello"));
        assert_eq!(outcome.bodies(), [BANNER, "ERROR folder"], "{folder}");
        assert_eq!(card.nodes.len(), 1);
    }
}

#[test]
fn admission_refuses_reading_opening_and_away_from_home() {
    let home = Facts { home: true, reader_open: false, opening: false };
    assert_eq!(admit(home), Ok(()));
    assert_eq!(admit(Facts { reader_open: true, ..home }), Err(Busy::Reading));
    assert_eq!(admit(Facts { opening: true, ..home }), Err(Busy::Opening));
    assert_eq!(admit(Facts { home: false, ..home }), Err(Busy::NotHome));
    assert_eq!(Busy::Reading.reply(), b"ERROR busy reason=reading");
}

#[test]
fn a_file_lands_whole_under_its_name() {
    // The runtime spike's largest file: 11 whole chunks and a partial one.
    let bytes = pattern(36_856);
    let mut card = FakeCard::default();
    let mut host = Host::new();
    host.put("dev", "runtime.bin", &bytes);
    let last_id = host.id;
    let outcome = lease(&mut card, &mut host);
    assert_eq!(outcome.end, End::Kept { last_id });
    let mut expected = vec![BANNER.to_owned(), format!("FILE free={}", 1u64 << 30)];
    expected.extend((1..=11).map(|n| format!("DATA {}", n * CHUNK_BYTES)).chain(["DATA 36856".into(), done(&bytes)]));
    assert_eq!(outcome.bodies(), expected);
    let ids: Vec<u32> = outcome.replies.iter().map(|&(id, _)| id).collect();
    assert_eq!(ids, (PUT_ID..=last_id).collect::<Vec<_>>(), "each reply answers its own request");
    assert_eq!(card.file(TARGET), Some(&bytes[..]));
    assert_eq!(card.file(TEMP), None);
}

#[test]
fn an_empty_file_is_written() {
    let mut card = FakeCard::default();
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", b""));
    assert_eq!(outcome.bodies()[1..], ["FILE free=1073741824".to_owned(), done(b"")]);
    assert_eq!(card.file(TARGET), Some(&b""[..]));
    assert_eq!(card.file(TEMP), None);
}

#[test]
fn short_card_writes_and_fragmented_replies_still_complete() {
    let bytes = pattern(5_131);
    let mut card = FakeCard { write_limit: 100, ..FakeCard::default() };
    let mut wire = FakeWire::new(std::mem::take(&mut Host::new().put("dev", "runtime.bin", &bytes).script));
    wire.write_limit = 7;
    let outcome = lease_with(&mut card, wire);
    assert_eq!(outcome.last(), done(&bytes));
    assert_eq!(card.file(TARGET), Some(&bytes[..]));
}

#[test]
fn an_existing_file_is_replaced_only_after_verification() {
    let (old, new) = (pattern(9_000), pattern(4_000));
    let mut card = FakeCard::default();
    card.put_file(TARGET, &old);
    card.put_file(TEMP, b"left by a power loss");
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &new));
    assert_eq!(outcome.last(), done(&new));
    assert_eq!(card.file(TARGET), Some(&new[..]));
    assert_eq!(card.file(TEMP), None);
}

#[test]
fn a_digest_mismatch_leaves_no_final_file() {
    let (old, bytes) = (b"old".to_vec(), pattern(4_000));
    let mut card = FakeCard::default();
    card.put_file(TARGET, &old);
    let mut host = Host::new();
    host.line(&format!("FILE dev runtime.bin {} {}", bytes.len(), hex(&sha256(b"other bytes"))))
        .data(&bytes).line("DONE");
    let last_id = host.id;
    let outcome = lease(&mut card, &mut host);
    assert_eq!(outcome.last(), "ERROR sha256-mismatch");
    assert_eq!(outcome.end, End::Kept { last_id });
    unchanged(&card, Some(&old));
}

#[test]
fn the_card_bytes_not_the_received_ones_are_checked() {
    let bytes = pattern(4_000);
    let mut card = FakeCard { flip: Some(3_500), ..FakeCard::default() };
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes));
    assert_eq!(outcome.last(), "ERROR sha256-mismatch");
    unchanged(&card, None);
}

#[test]
fn done_before_every_byte_is_short() {
    let bytes = pattern(4_000);
    let mut card = FakeCard::default();
    let outcome = lease(&mut card, Host::new().file("dev", "runtime.bin", &bytes).data(&bytes[..3_072]).line("DONE"));
    assert_eq!(outcome.last(), "ERROR short");
    unchanged(&card, None);
}

/// A transfer one chunk in, then `event`; the old file must survive it.
fn interrupted(event: Event) -> (FakeCard, Outcome) {
    let bytes = pattern(8_000);
    let mut card = FakeCard::default();
    card.put_file(TARGET, b"old");
    let outcome = lease(&mut card, Host::new().file("dev", "runtime.bin", &bytes).data(&bytes[..3_072]).then(event));
    (card, outcome)
}

#[test]
fn physical_input_abandons_and_keeps_the_old_file() {
    let (card, outcome) = interrupted(Event::Press);
    assert_eq!(outcome.end, End::Cancelled);
    assert_eq!(outcome.last(), "ERROR abandoned reason=physical-input");
    unchanged(&card, Some(b"old"));
}

#[test]
fn a_disconnect_abandons_the_transfer() {
    let (card, outcome) = interrupted(Event::Unplug);
    assert_eq!(outcome.end, End::Cancelled);
    assert!(!outcome.last().starts_with("ERROR"), "nothing is sent to a host that left");
    unchanged(&card, Some(b"old"));
}

#[test]
fn silence_times_out_and_removes_the_temp() {
    let (card, outcome) = interrupted(Event::Line(String::new()));
    assert_eq!(outcome.end, End::Cancelled);
    assert_eq!(outcome.last(), "ERROR abandoned reason=timeout");
    assert!((1_000 + IDLE_MS..1_000 + IDLE_MS + 10).contains(&outcome.wire.clock));
    unchanged(&card, Some(b"old"));
}

#[test]
fn a_hello_ends_the_lease_as_a_new_handshake() {
    let (card, outcome) = interrupted(Event::Line("N1 HELLO\n".into()));
    assert_eq!(outcome.end, End::Hello);
    unchanged(&card, Some(b"old"));
}

#[test]
fn out_of_session_lines_cancel() {
    for (line, reason) in [(format!("N1 {BOOT} {} {} DONE\n", SESSION + 1, PUT_ID + 1), "session"),
        (format!("N1 {BOOT} {SESSION} {PUT_ID} DONE\n"), "stale-id"),
        (format!("N1 {BOOT} {SESSION} {} DONE extra\n", PUT_ID + 1), "syntax"),
        ("N1 \u{7f}\n".into(), "line")] {
        let (card, outcome) = interrupted(Event::Line(line));
        assert_eq!(outcome.end, End::Cancelled, "{reason}");
        assert_eq!(outcome.last(), format!("ERROR abandoned reason={reason}"));
        unchanged(&card, Some(b"old"));
    }
}

#[test]
fn bad_chunks_are_refused() {
    let bytes = pattern(4_000);
    let whole = base64(&bytes[..3_072]);
    for (data, reason) in [(format!("DATA 1 {whole}"), "offset"), (format!("DATA 0 {}", base64(&bytes[..3_000])), "length"),
        (format!("DATA 0 *{}", &whole[1..]), "encoding"), ("DATA 0 QR==".into(), "encoding"),
        ("DATA 0".into(), "arguments")] {
        let mut card = FakeCard::default();
        card.put_file(TARGET, b"old");
        let outcome = lease(&mut card, Host::new().file("dev", "runtime.bin", &bytes).line(&data));
        assert_eq!(outcome.last(), format!("ERROR {reason}"), "{data:.20}");
        unchanged(&card, Some(b"old"));
    }
    let mut card = FakeCard::default();
    let outcome = lease(&mut card, Host::new().data(&bytes));
    assert_eq!(outcome.last(), "ERROR order");
}

#[test]
fn the_size_cap_bounds_one_file() {
    assert!(CAP_BYTES as usize >= 36_856, "the runtime spike's largest file fits");
    let digest = hex(&sha256(b""));
    let mut card = FakeCard::default();
    let outcome = lease(&mut card, Host::new().line(&format!("FILE dev runtime.bin {} {digest}", CAP_BYTES + 1)));
    assert_eq!(outcome.last(), "ERROR size");
    assert_eq!(card.nodes.len(), 1);
    let mut card = FakeCard::default();
    let outcome = lease(&mut card, Host::new().line(&format!("FILE dev runtime.bin {CAP_BYTES} {digest}")));
    assert_eq!(outcome.bodies()[1], "FILE free=1073741824");
    assert_eq!(outcome.last(), "ERROR abandoned reason=timeout");
    unchanged(&card, None);
}

#[test]
fn short_free_space_is_refused_before_any_write() {
    let bytes = pattern(5_000);
    let free = bytes.len() as u64 + RESERVE_BYTES - 1;
    let mut card = FakeCard { free, ..FakeCard::default() };
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes));
    assert_eq!(outcome.bodies(), [BANNER.to_owned(), format!("ERROR card-full free={free}")]);
    unchanged(&card, None);
    card.free += 1;
    assert_eq!(lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes)).last(), done(&bytes));
}

#[test]
fn card_failures_are_reported_with_their_step() {
    let bytes = pattern(4_000);
    for step in ["free", "create", "write", "sync", "rename"] {
        let mut card = FakeCard { fail: Some(step), ..FakeCard::default() };
        card.put_file(TARGET, b"old");
        let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes));
        assert_eq!(outcome.last(), format!("ERROR card step={step} code={FAILED}"));
        assert_eq!(card.file(TEMP), None, "{step}");
    }
    let mut card = FakeCard::default();
    for folder in ["/newt-sd/Newt", DEV, TARGET] { card.nodes.insert(folder.into(), Node::Folder); }
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes));
    assert_eq!(outcome.last(), "ERROR target", "a folder of that name is never written over");
}

#[test]
fn a_stalled_reply_cancels() {
    let mut card = FakeCard::default();
    let mut wire = FakeWire::new(std::mem::take(&mut Host::new().put("dev", "runtime.bin", b"x").script));
    wire.stalled = true;
    let outcome = lease_with(&mut card, wire);
    assert_eq!(outcome.end, End::Cancelled);
    assert!(outcome.replies.is_empty());
    unchanged(&card, None);
}
