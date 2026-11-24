// SPDX-License-Identifier: MIT
//! The bridge's card write (newt_core::dev_put) over a fake card and host:
//! the name and folder rules, admission, and that the final name only ever
//! holds the old file or the whole verified new one, whatever ends the lease.
//! commit.rs covers DONE's commit and the recovery an interrupted one needs.
mod commit;
mod fakes;
mod sha256;
mod receive_owner;

use fakes::{Event, FakeCard, FakeWire, HostHash, Node, FAILED};
use newt_core::dev_put::{self, admit, name_allowed, Busy, End, Facts, Session, CHUNK_BYTES, IDLE_MS,
    LINE_BYTES, MEMORY_BYTES, FRAME_BYTES};
use sha256::sha256;

const BOOT: u32 = 7;
const SESSION: u32 = 0x9000_0001;
const PUT_ID: u32 = 0x8000_0010;
const DEV: &str = "/newt-sd/Newt/Dev";
const TARGET: &str = "/newt-sd/Newt/Dev/runtime.bin";
const TEMP: &str = "/newt-sd/NEWT.PUT";
const OLD: &str = "/newt-sd/NEWT.OLD";
const MARKER: &str = "/newt-sd/NEWT.MRK";
const OTHER: &str = "/newt-sd/Newt/Dev/other.bin";
const BANNER: &str = "PUT line=4351 chunk=4096 path=hex data=raw";

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
        self.line(&file_line(folder, name, bytes.len() as u64, &hex(&sha256(bytes))))
    }
    fn data(&mut self, bytes: &[u8]) -> &mut Self {
        for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            self.line(&format!("DATA {} {}", index * CHUNK_BYTES, chunk.len()));
            self.script.push(Event::Bytes(chunk.to_vec()));
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
    wire.pressed = card.pressed.clone();
    wire.late = card.late.clone();
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

fn done(bytes: &[u8]) -> String { format!("DONE size={} sha256={}", bytes.len(), hex(&sha256(bytes))) }

/// After a lease that wrote nothing: no temp, and the target as it was.
fn unchanged(card: &FakeCard, old: Option<&[u8]>) {
    assert_eq!(card.file(TEMP), None, "the temp is removed");
    assert_eq!(card.file(TARGET), old, "the final name keeps what it held");
}

/// No backup or marker left: every replacement finished or was undone.
fn resolved(card: &FakeCard) {
    assert_eq!((card.file(OLD), card.file(MARKER)), (None, None));
}

fn file_line(folder: &str, name: &str, size: u64, digest: &str) -> String {
    let directory = match folder { "dev" => "Newt/Dev", "apps" => "Newt/Apps", "update" => "Newt/Update", other => other };
    let target = if directory.is_empty() { name.to_owned() } else { format!("{directory}/{name}") };
    format!("FILE {} {size} {digest}", hex(target.as_bytes()))
}

#[test]
fn names_outside_the_safe_set_are_refused() {
    for bad in [&b""[..], b"..", b"../x", b"a/b", b"/abs", b"a\\b", b"a..b", b"x\0",
        &[b'a'; 256], b"a.", b"a.txt ", b"CON", b"nul.txt", b"LPT9", b"x:y", b"a?b", b"x\x7f"] {
        assert!(!name_allowed(bad), "{bad:?}");
    }
    for good in [&b"newt.bin"[..], b"app-1_x.txt", b"0", b".hidden", b"a b", "caf\u{e9}".as_bytes(), &[b'a'; 255]] {
        assert!(name_allowed(good), "{good:?}");
    }
    for target in ["..", "../Books/a", "Newt/../x", "/absolute", "a//b", "a/", "./x", "a\\b", "C:/x", ".NEWT-PUT", ".newt-old/x", ".newt-old-name", "NEWT.PUT", "newt.old", "Newt.Mrk", "a/NEWT.PUT", "a/nul.txt", "bad."] {
        let mut card = FakeCard::default();
        let outcome = lease(&mut card, Host::new().file("", target, b""));
        assert_eq!(outcome.bodies(), [BANNER, "ERROR name"], "{target}");
        assert_eq!(card.nodes.len(), 1, "a refused path touches nothing");
    }
}

#[test]
fn any_valid_card_relative_destination_is_supported() {
    for target in ["root.txt", "Newt/Books/generated.epub", "another/deep/subfolder/a b.txt", "Newt/State/.hidden", "caf\u{e9}/reading.txt"] {
        let mut card = FakeCard::default();
        let outcome = lease(&mut card, Host::new().put("", target, b"hello"));
        assert_eq!(outcome.last(), done(b"hello"), "{target}");
        assert_eq!(card.file(&format!("/newt-sd/{target}")), Some(&b"hello"[..]));
    }
    let mut card = FakeCard::default();
    card.put_file("/newt-sd/not-a-folder", b"existing");
    assert_eq!(lease(&mut card, Host::new().put("not-a-folder", "x", b"new")).last(), "ERROR target");
    assert_eq!(card.file("/newt-sd/not-a-folder"), Some(&b"existing"[..]));
}

#[test]
fn paths_are_bounded_and_decoded_before_card_access() {
    let target = format!("{}/{}", "a".repeat(255), "b".repeat(246));
    assert_eq!(format!("/newt-sd/{target}").len(), 511);
    let mut card = FakeCard::default();
    assert_eq!(lease(&mut card, Host::new().put("", &target, b"whole")).last(), done(b"whole"));
    for encoded in [hex(format!("{target}b").as_bytes()), "f".into(), "zz".into(), "ff".into(), "610062".into()] {
        let mut card = FakeCard::default();
        let outcome = lease(&mut card, Host::new().line(&format!("FILE {encoded} 0 {}", hex(&sha256(b"")))));
        assert_eq!(outcome.bodies(), [BANNER, "ERROR name"]);
        assert_eq!(card.nodes.len(), 1);
    }
}

#[test]
fn paths_cannot_follow_non_directory_parents() {
    let mut card = FakeCard::default();
    card.nodes.insert("/newt-sd/link".into(), Node::Other);
    assert_eq!(lease(&mut card, Host::new().put("link/deep", "x", b"new")).last(), "ERROR target");
    assert_eq!(card.nodes.len(), 2);
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
    // Full blocks and a partial block have separately acknowledged offsets.
    let mut bytes = pattern(36_856);
    bytes[..11].copy_from_slice(b"N1 HELLO\n\0\r");
    let mut card = FakeCard::default();
    let mut host = Host::new();
    host.put("dev", "runtime.bin", &bytes);
    let last_id = host.id;
    let outcome = lease(&mut card, &mut host);
    assert_eq!(outcome.end, End::Kept { last_id });
    let mut expected = vec![BANNER.to_owned(), format!("FILE free={}", 1u64 << 30)];
    expected.extend((1..=bytes.len() / CHUNK_BYTES).map(|n| format!("DATA {}", n * CHUNK_BYTES)).chain(["DATA 36856".into(), done(&bytes)]));
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
    let (old, new) = (pattern(9_000), pattern(CHUNK_BYTES + 100));
    let mut card = FakeCard::default();
    card.put_file(TARGET, &old);
    card.put_file(TEMP, b"left by an abandoned transfer");
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &new));
    assert_eq!(outcome.last(), done(&new));
    assert_eq!(card.file(TARGET), Some(&new[..]));
    assert_eq!(card.file(TEMP), None);
    resolved(&card);
}

#[test]
fn every_lease_frame_is_bounded() {
    // The longest DATA line: 10-digit boot, session and ID, an offset near the cap.
    let line = format!("N1 {0} {0} {0} DATA {1} {2}\n", u32::MAX, u64::MAX - 1, CHUNK_BYTES);
    assert!(line.len() + CHUNK_BYTES <= FRAME_BYTES && LINE_BYTES < FRAME_BYTES, "{}", line.len());
    let (card, outcome) = interrupted(Event::Line(format!("{}\n", "A".repeat(LINE_BYTES + 1))));
    assert_eq!(outcome.last(), "ERROR abandoned reason=line", "a longer line is never taken");
    unchanged(&card, Some(b"old"));
}

#[test]
fn a_digest_mismatch_leaves_no_final_file() {
    let (old, bytes) = (b"old".to_vec(), pattern(CHUNK_BYTES + 100));
    let mut card = FakeCard::default();
    card.put_file(TARGET, &old);
    let mut host = Host::new();
    host.line(&file_line("dev", "runtime.bin", bytes.len() as u64, &hex(&sha256(b"other bytes"))))
        .data(&bytes).line("DONE");
    let last_id = host.id;
    let outcome = lease(&mut card, &mut host);
    assert_eq!(outcome.last(), "ERROR sha256-mismatch");
    assert_eq!(outcome.end, End::Kept { last_id });
    unchanged(&card, Some(&old));
}

#[test]
fn the_card_bytes_not_the_received_ones_are_checked() {
    let bytes = pattern(CHUNK_BYTES + 100);
    let mut card = FakeCard { flip: Some(3_500), ..FakeCard::default() };
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes));
    assert_eq!(outcome.last(), "ERROR sha256-mismatch");
    unchanged(&card, None);
}

#[test]
fn done_before_every_byte_is_short() {
    let bytes = pattern(CHUNK_BYTES + 100);
    let mut card = FakeCard::default();
    let outcome = lease(&mut card, Host::new().file("dev", "runtime.bin", &bytes).data(&bytes[..CHUNK_BYTES]).line("DONE"));
    assert_eq!(outcome.last(), "ERROR short");
    unchanged(&card, None);
}

/// A transfer two chunks in, then `event`; the old file must survive it.
fn interrupted(event: Event) -> (FakeCard, Outcome) {
    let bytes = pattern(8_000);
    let mut card = FakeCard::default();
    card.put_file(TARGET, b"old");
    let outcome = lease(&mut card, Host::new().file("dev", "runtime.bin", &bytes).data(&bytes[..CHUNK_BYTES]).then(event));
    (card, outcome)
}

#[test]
fn a_raw_block_has_a_nonrenewing_deadline() {
    let bytes = pattern(CHUNK_BYTES);
    let mut card = FakeCard::default();
    card.put_file(TARGET, b"old");
    let mut host = Host::new();
    host.file("dev", "runtime.bin", &bytes).line(&format!("DATA 0 {CHUNK_BYTES}"))
        .then(Event::Bytes(bytes[..32].to_vec())).then(Event::Wait(IDLE_MS - 10))
        .then(Event::Bytes(bytes[32..64].to_vec())).then(Event::Wait(20))
        .then(Event::Bytes(bytes[64..].to_vec())).line("DONE");
    let outcome = lease(&mut card, &mut host);
    assert_eq!(outcome.end, End::Cancelled);
    assert_eq!(outcome.last(), "ERROR abandoned reason=timeout");
    unchanged(&card, Some(b"old"));
}

#[test]
fn an_interrupted_raw_block_never_replaces_the_final_file() {
    for (event, reason) in [(Event::Press, Some("physical-input")), (Event::Unplug, None), (Event::Line(String::new()), Some("timeout"))] {
        let mut card = FakeCard::default();
        card.put_file(TARGET, b"old");
        let mut host = Host::new();
        host.file("dev", "runtime.bin", &pattern(CHUNK_BYTES))
            .line(&format!("DATA 0 {CHUNK_BYTES}"))
            .then(Event::Bytes(pattern(CHUNK_BYTES / 2))).then(event);
        let outcome = lease(&mut card, &mut host);
        assert_eq!(outcome.end, End::Cancelled);
        if let Some(reason) = reason { assert_eq!(outcome.last(), format!("ERROR abandoned reason={reason}")); }
        assert_eq!(card.count("write"), 0, "never store an incomplete block");
        unchanged(&card, Some(b"old"));
        resolved(&card);
    }
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
    let bytes = pattern(CHUNK_BYTES + 100);
    for (data, reason) in [(format!("DATA 1 {CHUNK_BYTES}"), "offset"),
        (format!("DATA 0 {}", CHUNK_BYTES - 1), "length"),
        ("DATA 0 0".into(), "length"), ("DATA 0 -1".into(), "length"),
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
fn free_card_space_is_the_size_limit() {
    let digest = hex(&sha256(b""));
    for size in [0x770001u64, 20_853_012, u32::MAX as u64 + 1, u64::MAX] {
        let mut card = FakeCard { free: size, ..FakeCard::default() };
        let outcome = lease(&mut card, Host::new().line(&file_line("dev", "runtime.bin", size, &digest)));
        assert_eq!(outcome.bodies()[1], format!("FILE free={size}"));
        assert_eq!(outcome.last(), "ERROR abandoned reason=timeout");
        unchanged(&card, None);
    }
}

#[test]
fn short_free_space_is_refused_before_any_write() {
    let bytes = pattern(5_000);
    let free = bytes.len() as u64 - 1;
    let mut card = FakeCard { free, ..FakeCard::default() };
    let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes));
    assert_eq!(outcome.bodies(), [BANNER.to_owned(), format!("ERROR card-full free={free}")]);
    unchanged(&card, None);
    card.free += 1;
    assert_eq!(lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes)).last(), done(&bytes));
}

#[test]
fn card_failures_are_reported_with_their_step() {
    let bytes = pattern(CHUNK_BYTES + 100);
    for (step, old) in [("free", true), ("create", true), ("write", true), ("sync", true), ("replace", true),
        ("rename", false)] {
        let mut card = FakeCard { fail: Some(if step == "replace" { "rename" } else { step }), ..FakeCard::default() };
        if old { card.put_file(TARGET, b"old"); }
        let outcome = lease(&mut card, Host::new().put("dev", "runtime.bin", &bytes));
        assert_eq!(outcome.last(), format!("ERROR card step={step} code={FAILED}"));
        unchanged(&card, old.then_some(b"old"));
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
