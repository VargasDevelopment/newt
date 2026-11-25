//! Core's receive deadline and the firmware reporter's output deadline.
//! The token's packet polling is checked separately in receive_owner.rs.
use super::*;
use newt_core::dev_put::Wire;

struct Output<'a> { wire: &'a mut FakeWire, connected_after: u32 }
impl Wire for Output<'_> {
    fn connected(&mut self) -> bool { self.wire.clock >= self.connected_after }
    fn read_byte(&mut self) -> i32 { self.wire.read_byte() }
    fn write(&mut self, bytes: &[u8]) -> usize { self.wire.write(bytes) }
    fn now(&mut self) -> u32 { self.wire.now() }
    fn physical_input(&mut self) -> bool { self.wire.physical_input() }
    fn pause(&mut self) { self.wire.pause(); }
}

#[test]
fn stalled_sender_exits_and_its_diagnostic_output_is_bounded() {
    for mode in ["short", "reconnect", "blocked", "gone"] {
        let mut card = FakeCard::default(); card.put_file(TARGET, b"old");
        let mut host = Host::new();
        host.file("dev", "runtime.bin", &pattern(CHUNK_BYTES))
            .line(&format!("DATA 0 {CHUNK_BYTES}"))
            .then(Event::Bytes(vec![0x5a; 17]));
        let mut outcome = lease(&mut card, &mut host);
        assert_eq!(outcome.end, End::Cancelled);
        assert_eq!(outcome.last(), "ERROR abandoned reason=timeout");
        assert!((1_000 + IDLE_MS..1_000 + IDLE_MS + 10).contains(&outcome.wire.clock));
        assert_eq!(card.count("write"), 0);
        unchanged(&card, Some(b"old"));
        let mut counters = observation::Counters::default();
        for event in &outcome.wire.observations { counters.observe(outcome.wire.clock, *event); }
        let mut row = String::new(); counters.row(outcome.wire.clock, &mut row).unwrap();
        assert!(row.starts_with("\nPUT_STATS reason=4 "));
        let sent = outcome.wire.sent.len();
        let start = outcome.wire.clock;
        outcome.wire.write_limit = 3;
        outcome.wire.stalled = mode == "blocked";
        let connected_after = match mode { "gone" => u32::MAX, "reconnect" => start + 5, _ => start };
        observation::report(&mut Output { wire: &mut outcome.wire, connected_after }, row.as_bytes());
        let elapsed = outcome.wire.clock - start;
        assert!(elapsed <= 1_000, "output never extends its existing 1 s budget");
        if mode == "blocked" || mode == "gone" {
            assert_eq!(elapsed, 1_000);
            assert_eq!(outcome.wire.sent.len(), sent);
        } else {
            assert_eq!(&outcome.wire.sent[sent..], row.as_bytes(), "{mode}: one complete row, no replay");
            if mode == "reconnect" { assert_eq!(elapsed, 5); }
        }
    }
}

struct CutReply { wire: FakeWire, counters: observation::Counters, cut: bool, reconnect_at: u32 }
impl Wire for CutReply {
    fn connected(&mut self) -> bool { self.wire.clock >= self.reconnect_at }
    fn read_byte(&mut self) -> i32 {
        let byte = self.wire.read_byte(); self.counters.read(byte); byte
    }
    fn write(&mut self, bytes: &[u8]) -> usize {
        if !self.cut && bytes.windows(6).any(|word| word == b"ERROR ") {
            self.cut = true; self.reconnect_at = self.wire.clock + 5;
            self.wire.write(&bytes[..bytes.len() / 2])
        } else { self.wire.write(bytes) }
    }
    fn now(&mut self) -> u32 { self.wire.now() }
    fn physical_input(&mut self) -> bool { self.wire.physical_input() }
    fn pause(&mut self) { self.wire.pause(); self.counters.paused(1); }
    fn observe(&mut self, event: dev_put::Observation) { self.counters.observe(self.wire.clock, event); }
}

#[test]
fn a_partial_timeout_reply_cannot_swallow_the_exit_row() {
    let mut host = Host::new();
    host.file("dev", "runtime.bin", &pattern(CHUNK_BYTES))
        .line(&format!("DATA 0 {CHUNK_BYTES}")).then(Event::Bytes(vec![0x5a; 17]));
    let mut port = CutReply { wire: FakeWire::new(host.script), counters: observation::Counters::default(),
        cut: false, reconnect_at: 0 };
    let mut card = FakeCard::default(); card.put_file(TARGET, b"old");
    let mut memory: Box<[u8; MEMORY_BYTES]> = vec![0; MEMORY_BYTES].try_into().unwrap();
    let session = Session { boot: BOOT, session: SESSION, request: PUT_ID };
    assert_eq!(dev_put::run(session, &mut memory, &mut port, &mut card, &mut HostHash::default()), End::Cancelled);
    assert!(port.cut);
    assert!((1_000 + IDLE_MS..1_000 + IDLE_MS + 10).contains(&port.wire.clock));
    let at = port.wire.sent.iter().rposition(|&byte| byte == b'\n').unwrap() + 1;
    let fragment = port.wire.sent[at..].to_vec();
    assert!(fragment.starts_with(b"!N1 ") && !fragment.contains(&b'\n'));
    let mut row = String::new(); port.counters.row(port.wire.clock, &mut row).unwrap();
    observation::report(&mut port, row.as_bytes());
    let lines: Vec<_> = port.wire.sent[at..].split(|&byte| byte == b'\n').collect();
    assert_eq!(lines.len(), 3, "one delimited fragment, one complete row, final LF");
    assert_eq!(lines[0], fragment.as_slice());
    assert!(lines[1].starts_with(b"PUT_STATS reason=4 "));
    assert!(lines[2].is_empty());
    assert_eq!([b"\n".as_slice(), lines[1], b"\n".as_slice()].concat(), row.as_bytes());
    assert_eq!(card.count("write"), 0);
    unchanged(&card, Some(b"old"));
}
